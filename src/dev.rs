use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use tracing::{error, info};

use crate::singleton::acquire_named_mutex;

const WATCH_EXTENSIONS: &[&str] = &["rs", "toml", "bat"];
const IGNORED_DIRS: &[&str] = &[
    ".git",
    ".venv",
    "__pycache__",
    ".pytest_cache",
    ".mypy_cache",
    ".ruff_cache",
    "target",
];
const IGNORED_PATH_PREFIXES: &[&[&str]] =
    &[&[".context", "chirp-stt", "src", "chirp", "assets", "models"]];
const DEV_SUPERVISOR_ENV: &str = "CHIRP_DEV_SUPERVISOR";
const DEV_RETRY_DELAY: Duration = Duration::from_millis(750);
const DEV_MUTEX_NAME: &str = "Local\\ChirpRustDevSingleton";

type Snapshot = BTreeMap<String, (u64, u64)>;

pub fn run_dev(project_root: &Path, interval: Duration, forwarded_args: &[String]) -> Result<()> {
    if interval.is_zero() {
        bail!("dev interval must be greater than zero");
    }

    if relaunch_dev_supervisor(project_root)? {
        return Ok(());
    }

    let _dev_mutex = acquire_named_mutex(
        DEV_MUTEX_NAME,
        "chirp-dev: another dev runner is already active",
    )?;

    let forwarded_args = normalize_forwarded_args(forwarded_args);
    let mut snapshot = snapshot_repo(project_root)?;
    let mut child = start_child_with_retry(project_root, &forwarded_args);

    info!("chirp-dev: watching repo for changes");

    loop {
        thread::sleep(interval.max(Duration::from_millis(100)));

        if let Some(status) = child
            .try_wait()
            .context("failed to poll dev child process")?
        {
            info!("chirp-dev: app exited with {status}; restarting");
            child = start_child_with_retry(project_root, &forwarded_args);
            snapshot = snapshot_repo(project_root)?;
            continue;
        }

        let new_snapshot = snapshot_repo(project_root)?;
        if let Some(changed) = detect_changes(&snapshot, &new_snapshot) {
            info!("chirp-dev: change detected in {changed}; restarting");
            stop_child(&mut child);
            child = start_child_with_retry(project_root, &forwarded_args);
            snapshot = new_snapshot;
        }
    }
}

fn relaunch_dev_supervisor(project_root: &Path) -> Result<bool> {
    if std::env::var_os(DEV_SUPERVISOR_ENV).is_some() {
        return Ok(false);
    }

    let current_exe = std::env::current_exe().context("failed to resolve current executable")?;
    let staged_exe = stage_dev_binary(&current_exe, project_root, "supervisor")?;
    let args = std::env::args_os().skip(1).collect::<Vec<_>>();

    let mut command = Command::new(&staged_exe);
    command
        .args(args)
        .env(DEV_SUPERVISOR_ENV, "1")
        .current_dir(project_root)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit());

    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
        command.creation_flags(CREATE_NEW_PROCESS_GROUP);
    }

    command.spawn().with_context(|| {
        format!(
            "failed to relaunch staged dev supervisor {}",
            staged_exe.display()
        )
    })?;
    Ok(true)
}

fn start_child_with_retry(project_root: &Path, forwarded_args: &[String]) -> Child {
    loop {
        match start_child(project_root, forwarded_args) {
            Ok(child) => return child,
            Err(error) => {
                error!("chirp-dev: start failed: {error:#}");
                error!(
                    "chirp-dev: retrying in {:.2}s",
                    DEV_RETRY_DELAY.as_secs_f32()
                );
                thread::sleep(DEV_RETRY_DELAY);
            }
        }
    }
}

fn normalize_forwarded_args(forwarded_args: &[String]) -> Vec<String> {
    let mut normalized = forwarded_args.to_vec();
    if normalized.first().map(String::as_str) == Some("--") {
        normalized.remove(0);
    }
    if normalized.is_empty() {
        normalized.push("run".to_string());
    }
    normalized
}

fn start_child(project_root: &Path, forwarded_args: &[String]) -> Result<Child> {
    let exe = build_app_binary(project_root)?;
    let staged_exe = stage_dev_binary(&exe, project_root, "child")?;
    let mut command = Command::new(&staged_exe);
    command
        .args(forwarded_args)
        .current_dir(project_root)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit());

    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
        command.creation_flags(CREATE_NEW_PROCESS_GROUP);
    }

    command
        .spawn()
        .with_context(|| format!("failed to start child process with args {forwarded_args:?}"))
}

fn build_app_binary(project_root: &Path) -> Result<PathBuf> {
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let status = Command::new(cargo)
        .arg("build")
        .current_dir(project_root)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .context("failed to start cargo build for dev child")?;
    if !status.success() {
        bail!("cargo build failed for dev child with status {status}");
    }

    let exe_name = if cfg!(target_os = "windows") {
        "chirpr-cli.exe"
    } else {
        "chirpr-cli"
    };
    Ok(project_root.join("target").join("debug").join(exe_name))
}

fn stage_dev_binary(exe: &Path, project_root: &Path, role: &str) -> Result<PathBuf> {
    let dev_root = project_root.join("target").join("dev-runner");
    fs::create_dir_all(&dev_root).with_context(|| {
        format!(
            "failed to create dev runner directory {}",
            dev_root.display()
        )
    })?;

    let binary_name = exe
        .file_name()
        .context("failed to resolve current executable name")?;
    let staged_path = dev_root.join(format!(
        "{}-{role}-{}.exe",
        binary_name.to_string_lossy().trim_end_matches(".exe"),
        std::process::id()
    ));

    fs::copy(exe, &staged_path).with_context(|| {
        format!(
            "failed to copy {} to {} for dev staging",
            exe.display(),
            staged_path.display()
        )
    })?;

    Ok(staged_path)
}

fn stop_child(child: &mut Child) {
    if child.try_wait().ok().flatten().is_some() {
        return;
    }

    let _ = child.kill();
    let _ = child.wait();
}

fn snapshot_repo(root: &Path) -> Result<Snapshot> {
    let mut snapshot = BTreeMap::new();
    for path in iter_watch_files(root)? {
        let relative = path
            .strip_prefix(root)
            .expect("watched file should live under root")
            .to_string_lossy()
            .replace('\\', "/");
        let metadata = match std::fs::metadata(&path) {
            Ok(value) => value,
            Err(_) => continue,
        };
        let modified = metadata
            .modified()
            .ok()
            .and_then(|value| value.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|value| value.as_nanos() as u64)
            .unwrap_or(0);
        snapshot.insert(relative, (modified, metadata.len()));
    }
    Ok(snapshot)
}

fn iter_watch_files(root: &Path) -> Result<Vec<PathBuf>> {
    let mut pending = vec![root.to_path_buf()];
    let mut files = Vec::new();

    while let Some(current) = pending.pop() {
        for entry in std::fs::read_dir(&current)
            .with_context(|| format!("failed to read directory {}", current.display()))?
        {
            let entry = entry?;
            let path = entry.path();
            let file_type = entry.file_type()?;

            if file_type.is_dir() {
                if should_ignore_dir(root, &path) {
                    continue;
                }
                pending.push(path);
            } else if file_type.is_file() && should_watch_file(root, &path) {
                files.push(path);
            }
        }
    }

    files.sort();
    Ok(files)
}

fn should_ignore_dir(root: &Path, path: &Path) -> bool {
    let relative_parts = relative_parts(root, path);
    relative_parts
        .iter()
        .any(|part| IGNORED_DIRS.contains(part))
        || IGNORED_PATH_PREFIXES
            .iter()
            .any(|prefix| relative_parts.starts_with(prefix))
}

fn should_watch_file(root: &Path, path: &Path) -> bool {
    let relative_parts = relative_parts(root, path);
    if relative_parts
        .iter()
        .any(|part| IGNORED_DIRS.contains(part))
    {
        return false;
    }
    if IGNORED_PATH_PREFIXES
        .iter()
        .any(|prefix| relative_parts.starts_with(prefix))
    {
        return false;
    }

    let Some(extension) = path.extension().and_then(OsStr::to_str) else {
        return false;
    };

    WATCH_EXTENSIONS.contains(&extension.to_ascii_lowercase().as_str())
}

fn detect_changes(old_snapshot: &Snapshot, new_snapshot: &Snapshot) -> Option<String> {
    let old_paths = old_snapshot.keys().cloned().collect::<BTreeSet<_>>();
    let new_paths = new_snapshot.keys().cloned().collect::<BTreeSet<_>>();

    old_paths
        .difference(&new_paths)
        .next()
        .cloned()
        .or_else(|| new_paths.difference(&old_paths).next().cloned())
        .or_else(|| {
            old_paths
                .intersection(&new_paths)
                .find(|path| old_snapshot.get(*path) != new_snapshot.get(*path))
                .cloned()
        })
}

fn relative_parts<'a>(root: &Path, path: &'a Path) -> Vec<&'a str> {
    path.strip_prefix(root)
        .ok()
        .into_iter()
        .flat_map(|relative| relative.iter())
        .filter_map(OsStr::to_str)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_forwarded_args_to_run() {
        assert_eq!(normalize_forwarded_args(&[]), vec!["run"]);
    }

    #[test]
    fn strips_separator_before_forwarding() {
        assert_eq!(
            normalize_forwarded_args(&["--".into(), "--verbose".into()]),
            vec!["--verbose"]
        );
    }

    #[test]
    fn detects_added_removed_and_modified_files() {
        let old = Snapshot::from([("a.rs".into(), (1, 10)), ("b.toml".into(), (2, 20))]);
        let added = Snapshot::from([
            ("a.rs".into(), (1, 10)),
            ("b.toml".into(), (2, 20)),
            ("c.rs".into(), (3, 30)),
        ]);
        assert_eq!(detect_changes(&old, &added).as_deref(), Some("c.rs"));

        let removed = Snapshot::from([("b.toml".into(), (2, 20))]);
        assert_eq!(detect_changes(&old, &removed).as_deref(), Some("a.rs"));

        let modified = Snapshot::from([("a.rs".into(), (9, 10)), ("b.toml".into(), (2, 20))]);
        assert_eq!(detect_changes(&old, &modified).as_deref(), Some("a.rs"));
    }

    #[test]
    fn ignores_target_and_model_paths() {
        let root = Path::new(r"E:\development\chirp\chirp-rust");
        assert!(!should_watch_file(
            root,
            &root.join("target").join("debug").join("foo.rs")
        ));
        assert!(!should_watch_file(
            root,
            &root
                .join(".context")
                .join("chirp-stt")
                .join("src")
                .join("chirp")
                .join("assets")
                .join("models")
                .join("model.onnx")
        ));
        assert!(should_watch_file(root, &root.join("src").join("main.rs")));
    }
}

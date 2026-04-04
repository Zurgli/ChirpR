use std::path::Path;
use std::process::Command;

use anyhow::{Context, Result};

const RUN_KEY: &str = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run";
const VALUE_NAME: &str = "ChirpRust";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AutostartAction {
    Enable,
    Disable,
    Status,
}

pub fn run(action: AutostartAction, exe_path: &Path) -> Result<()> {
    #[cfg(target_os = "windows")]
    {
        match action {
            AutostartAction::Enable => enable(exe_path),
            AutostartAction::Disable => disable(),
            AutostartAction::Status => status(),
        }
    }

    #[cfg(not(target_os = "windows"))]
    {
        let _ = action;
        let _ = exe_path;
        anyhow::bail!("autostart is only supported on Windows")
    }
}

#[cfg(target_os = "windows")]
fn enable(exe_path: &Path) -> Result<()> {
    let command_value = autostart_command_value(exe_path);
    let status = Command::new("reg")
        .args([
            "add",
            RUN_KEY,
            "/v",
            VALUE_NAME,
            "/t",
            "REG_SZ",
            "/d",
            &command_value,
            "/f",
        ])
        .status()
        .context("failed to invoke reg add for autostart")?;

    if !status.success() {
        anyhow::bail!("failed to enable autostart");
    }

    println!("autostart enabled for {}", exe_path.display());
    Ok(())
}

#[cfg(target_os = "windows")]
fn autostart_command_value(exe_path: &Path) -> String {
    let launch_path = resolve_autostart_executable(exe_path);
    let uses_cli = launch_path
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.eq_ignore_ascii_case("chirpr-cli.exe"));

    if uses_cli {
        format!("\"{}\" run", launch_path.display())
    } else {
        format!("\"{}\"", launch_path.display())
    }
}

#[cfg(target_os = "windows")]
fn resolve_autostart_executable(exe_path: &Path) -> std::path::PathBuf {
    let is_cli = exe_path
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.eq_ignore_ascii_case("chirpr-cli.exe"));

    if is_cli {
        if let Some(parent) = exe_path.parent() {
            let launcher = parent.join("chirpr.exe");
            if launcher.is_file() {
                return launcher;
            }
        }
    }

    exe_path.to_path_buf()
}

#[cfg(target_os = "windows")]
fn disable() -> Result<()> {
    let status = Command::new("reg")
        .args(["delete", RUN_KEY, "/v", VALUE_NAME, "/f"])
        .status()
        .context("failed to invoke reg delete for autostart")?;

    if !status.success() {
        anyhow::bail!("failed to disable autostart");
    }

    println!("autostart disabled");
    Ok(())
}

#[cfg(target_os = "windows")]
fn status() -> Result<()> {
    let output = Command::new("reg")
        .args(["query", RUN_KEY, "/v", VALUE_NAME])
        .output()
        .context("failed to invoke reg query for autostart")?;

    if output.status.success() {
        let stdout = String::from_utf8_lossy(&output.stdout);
        println!("autostart enabled");
        println!("{}", stdout.trim());
    } else {
        println!("autostart disabled");
    }

    Ok(())
}

#[cfg(all(test, target_os = "windows"))]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn unique_temp_dir(name: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("valid system time")
            .as_nanos();
        std::env::temp_dir().join(format!("chirpr-{name}-{nanos}"))
    }

    #[test]
    fn autostart_prefers_launcher_when_enabled_from_cli() {
        let root = unique_temp_dir("autostart-launcher");
        fs::create_dir_all(&root).unwrap();
        let launcher = root.join("chirpr.exe");
        fs::write(&launcher, "").unwrap();
        let cli = root.join("chirpr-cli.exe");

        let command = autostart_command_value(&cli);

        assert_eq!(command, format!("\"{}\"", launcher.display()));
    }

    #[test]
    fn autostart_keeps_run_subcommand_for_cli_only_bundle() {
        let root = unique_temp_dir("autostart-cli");
        fs::create_dir_all(&root).unwrap();
        let cli = root.join("chirpr-cli.exe");

        let command = autostart_command_value(&cli);

        assert_eq!(command, format!("\"{}\" run", cli.display()));
    }
}

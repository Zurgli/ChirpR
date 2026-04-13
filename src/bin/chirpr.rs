#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

use chirp_rust::config::ProjectPaths;
use chirp_rust::logger;
use clap::Parser;

#[derive(Debug, Parser)]
#[command(name = "chirpr", version, about = "ChirpR background launcher")]
struct LauncherCli {
    #[arg(short, long)]
    verbose: bool,

    #[arg(long)]
    config: Option<std::path::PathBuf>,

    #[arg(long)]
    settings: bool,
}

fn main() {
    if let Err(error) = run() {
        tracing::error!("{error:#}");
        #[cfg(target_os = "windows")]
        unsafe {
            use std::ffi::CString;
            use windows_sys::Win32::Foundation::HWND;
            use windows_sys::Win32::UI::WindowsAndMessaging::{MB_ICONERROR, MB_OK, MessageBoxA};

            let title = CString::new("ChirpR").expect("valid title");
            let message = CString::new(format!("ChirpR failed to start:\n\n{error:#}"))
                .expect("valid message");
            MessageBoxA(
                HWND::default(),
                message.as_ptr().cast(),
                title.as_ptr().cast(),
                MB_OK | MB_ICONERROR,
            );
        }
        std::process::exit(1);
    }
}

fn run() -> anyhow::Result<()> {
    let cli = LauncherCli::parse();
    logger::init(cli.verbose);
    let mut paths = ProjectPaths::discover()?;
    if let Some(config_path) = cli.config {
        paths = paths.with_config_path(config_path);
    }
    if cli.settings {
        chirp_rust::settings::run(paths)
    } else {
        chirp_rust::run_background_app(paths)
    }
}

#[cfg(test)]
mod tests {
    use super::LauncherCli;
    use clap::Parser;

    #[test]
    fn parses_settings_flag() {
        let cli = LauncherCli::parse_from(["chirpr", "--settings"]);
        assert!(cli.settings);
        assert!(!cli.verbose);
    }

    #[test]
    fn parses_settings_with_config_path() {
        let cli = LauncherCli::parse_from(["chirpr", "--settings", "--config", "custom.toml"]);
        assert!(cli.settings);
        assert_eq!(
            cli.config.as_deref(),
            Some(std::path::Path::new("custom.toml"))
        );
    }
}

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
    let command_value = format!("\"{}\" run", exe_path.display());
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

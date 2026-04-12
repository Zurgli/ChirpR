pub mod app;
pub mod audio;
pub mod audio_feedback;
pub mod autostart;
pub mod cli;
pub mod config;
pub mod dev;
pub mod keyboard;
pub mod logger;
pub mod recording;
pub mod recording_overlay;
pub mod settings;
pub mod singleton;
pub mod stt;
pub mod text_injection;
pub mod text_processing;

use std::thread;
use std::time::Duration;

use anyhow::{Result, bail};
use config::ProjectPaths;
use singleton::{terminate_other_app_instances, try_acquire_named_mutex};

const APP_MUTEX_NAME: &str = "Local\\ChirpRustAppSingleton";

pub fn run_background_app(paths: ProjectPaths) -> Result<()> {
    recording_overlay::enable_dpi_awareness();
    terminate_other_app_instances()?;

    // The previous instance may still be releasing the singleton mutex briefly after TerminateProcess.
    const RETRY_MS: u64 = 100;
    const MAX_WAIT_MS: u64 = 5000;
    let mut waited = 0u64;
    let _app_mutex = loop {
        match try_acquire_named_mutex(APP_MUTEX_NAME)? {
            Some(guard) => break guard,
            None => {
                if waited >= MAX_WAIT_MS {
                    bail!("chirp-rust: another app instance is still running; close it or try again in a moment.");
                }
                thread::sleep(Duration::from_millis(RETRY_MS));
                waited += RETRY_MS;
            }
        }
    };

    let app = app::ChirpApp::new(paths)?;
    app.run()
}

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
pub mod singleton;
pub mod stt;
pub mod text_injection;
pub mod text_processing;

use anyhow::Result;
use config::ProjectPaths;
use singleton::acquire_named_mutex;

const APP_MUTEX_NAME: &str = "Local\\ChirpRustAppSingleton";

pub fn run_background_app(paths: ProjectPaths) -> Result<()> {
    recording_overlay::enable_dpi_awareness();
    let _app_mutex = acquire_named_mutex(
        APP_MUTEX_NAME,
        "chirp-rust: another app instance is already active",
    )?;
    let app = app::ChirpApp::new(paths)?;
    app.run()
}

use std::sync::{Arc, Mutex};
use std::thread;

use anyhow::Result;

use crate::audio::AudioBuffer;
use crate::audio_feedback::AudioFeedback;
use crate::config::{ChirpConfig, ProjectPaths};
use crate::keyboard::{KeyboardController, KeyboardShortcutListener};
use crate::recording::{ActiveRecording, CaptureSummary, MicrophoneRecorder};
use crate::recording_overlay::RecordingOverlay;
use crate::stt::parakeet::{ParakeetManager, ParakeetModelSpec};
use crate::text_injection::TextInjector;
use crate::text_processing::TextProcessor;

struct AppState {
    active_recording: Option<ActiveRecording>,
}

pub struct ChirpApp {
    config: ChirpConfig,
    paths: ProjectPaths,
    keyboard: Arc<KeyboardController>,
    shortcut_listener: KeyboardShortcutListener,
    overlay: Arc<Mutex<RecordingOverlay>>,
    audio_feedback: AudioFeedback,
    parakeet: Arc<Mutex<ParakeetManager>>,
    state: Mutex<AppState>,
}

impl ChirpApp {
    pub fn new(paths: ProjectPaths) -> Result<Self> {
        let config = ChirpConfig::load(&paths)?;
        let model_dir = paths.model_dir(
            &config.parakeet_model,
            config.parakeet_quantization.as_deref(),
        )?;
        let spec = ParakeetModelSpec::resolve(
            &config.parakeet_model,
            config.parakeet_quantization.as_deref(),
        )?;
        let keyboard = Arc::new(KeyboardController::new()?);
        let shortcut_listener = KeyboardShortcutListener::register(&config.primary_shortcut)?;
        let parakeet = Arc::new(Mutex::new(spec.create_manager(&model_dir)?));
        Ok(Self {
            audio_feedback: AudioFeedback::new(
                config.audio_feedback,
                config.audio_feedback_volume,
                paths.assets_root.join("sounds"),
            ),
            overlay: Arc::new(Mutex::new(RecordingOverlay::new(config.recording_overlay))),
            config,
            paths,
            keyboard,
            shortcut_listener,
            parakeet,
            state: Mutex::new(AppState {
                active_recording: None,
            }),
        })
    }

    pub fn run(&self) -> Result<()> {
        println!(
            "Chirp ready. Toggle recording with {}",
            self.config.primary_shortcut
        );

        loop {
            self.shortcut_listener.recv()?;
            self.toggle_recording()?;
        }
    }

    fn toggle_recording(&self) -> Result<()> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| anyhow::anyhow!("app state lock poisoned"))?;

        if state.active_recording.is_none() {
            match MicrophoneRecorder::start_default() {
                Ok(recording) => {
                    state.active_recording = Some(recording);
                    drop(state);
                    self.audio_feedback
                        .play_start(self.config.start_sound_path.as_deref());
                    if let Ok(overlay) = self.overlay.lock() {
                        overlay.show("transcribing");
                    }
                    println!("Recording started");
                }
                Err(error) => {
                    drop(state);
                    self.audio_feedback
                        .play_error(self.config.error_sound_path.as_deref());
                    return Err(error);
                }
            }
        } else {
            let recording = state.active_recording.take().expect("recording present");
            drop(state);
            if let Ok(overlay) = self.overlay.lock() {
                overlay.hide();
            }
            self.audio_feedback
                .play_stop(self.config.stop_sound_path.as_deref());
            self.spawn_transcription(recording);
            println!("Recording stopped");
        }

        Ok(())
    }

    fn spawn_transcription(&self, recording: ActiveRecording) {
        let config = self.config.clone();
        let paths = self.paths.clone();
        let overlay = Arc::clone(&self.overlay);
        let keyboard = Arc::clone(&self.keyboard);
        let parakeet = Arc::clone(&self.parakeet);

        thread::spawn(move || {
            let processor =
                TextProcessor::new(config.word_overrides.clone(), &config.post_processing);
            let injector = TextInjector::new(
                keyboard,
                processor,
                &config.injection_mode,
                &config.paste_mode,
                config.clipboard_behavior,
                config.clipboard_clear_delay,
            );

            let recording = match recording.stop() {
                Ok(value) => value,
                Err(error) => {
                    eprintln!("error: failed to stop recording: {error:#}");
                    return;
                }
            };

            if let Ok(overlay) = overlay.lock() {
                overlay.show("loading");
            }

            match transcribe_capture(
                &paths,
                &config,
                &recording.audio,
                Some(&recording.summary),
                Some(&parakeet),
            ) {
                Ok(text) => {
                    if !text.trim().is_empty() {
                        if let Err(error) = injector.inject(&text) {
                            eprintln!("error: text injection failed: {error:#}");
                        }
                    }
                }
                Err(error) => {
                    eprintln!("error: transcription failed: {error:#}");
                }
            }

            if let Ok(overlay) = overlay.lock() {
                overlay.hide();
            }
        });
    }
}

pub fn transcribe_capture(
    paths: &ProjectPaths,
    config: &ChirpConfig,
    source_audio: &AudioBuffer,
    _summary: Option<&CaptureSummary>,
    manager: Option<&Arc<Mutex<ParakeetManager>>>,
) -> Result<String> {
    if source_audio.mono_samples.is_empty() {
        return Ok(String::new());
    }

    let audio = source_audio.resample_to(16_000)?;
    let decode = if let Some(manager) = manager {
        let mut manager = manager
            .lock()
            .map_err(|_| anyhow::anyhow!("parakeet manager lock poisoned"))?;
        manager.maybe_unload();
        manager.greedy_decode_waveform(&audio.mono_samples, 10)?
    } else {
        let model_dir = paths.model_dir(
            &config.parakeet_model,
            config.parakeet_quantization.as_deref(),
        )?;
        let spec = ParakeetModelSpec::resolve(
            &config.parakeet_model,
            config.parakeet_quantization.as_deref(),
        )?;
        let mut manager = spec.create_manager(&model_dir)?;
        manager.greedy_decode_waveform(&audio.mono_samples, 10)?
    };
    Ok(decode.text)
}

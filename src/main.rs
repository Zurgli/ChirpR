use anyhow::Result;
use chirp_rust::app::{ChirpApp, transcribe_capture};
use chirp_rust::audio::AudioBuffer;
use chirp_rust::cli::{Cli, Command};
use chirp_rust::config::{ChirpConfig, ProjectPaths};
use chirp_rust::recording::{ActiveRecording, MicrophoneRecorder};
use chirp_rust::stt::parakeet::ParakeetModelSpec;
use chirp_rust::text_processing::TextProcessor;
use clap::Parser;
use std::io::{self, BufRead};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;
use std::time::Instant;

fn main() {
    if let Err(error) = run() {
        eprintln!("error: {error:#}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let cli = Cli::parse();
    let mut paths = ProjectPaths::discover()?;
    if let Some(config_path) = cli.config.as_ref() {
        paths = paths.with_config_path(config_path.clone());
    }

    match cli.command.clone().unwrap_or(Command::Check) {
        Command::Setup => {
            let config = ChirpConfig::load(&paths)?;
            paths.ensure_models_root()?;
            let model_dir = paths.model_dir(
                &config.parakeet_model,
                config.parakeet_quantization.as_deref(),
            )?;
            let spec = ParakeetModelSpec::resolve(
                &config.parakeet_model,
                config.parakeet_quantization.as_deref(),
            )?;
            let downloaded_files = spec.ensure_downloaded(&model_dir)?;
            println!("model ready at {}", model_dir.display());
            println!("downloaded {} required files", downloaded_files.len());
        }
        Command::Check => {
            let config = ChirpConfig::load(&paths)?;
            let model_dir = paths.model_dir(
                &config.parakeet_model,
                config.parakeet_quantization.as_deref(),
            )?;
            let spec = ParakeetModelSpec::resolve(
                &config.parakeet_model,
                config.parakeet_quantization.as_deref(),
            )?;
            let missing_files = spec.missing_files(&model_dir);
            let processor =
                TextProcessor::new(config.word_overrides.clone(), &config.post_processing);
            let processed_sample = processor.process("test");
            println!("config OK");
            println!("model dir: {}", model_dir.display());
            if missing_files.is_empty() {
                println!("model files: ready");
                let manager = spec.create_manager(&model_dir)?;
                println!("onnx sessions: ready");
                if cli.verbose {
                    let bundle = manager.load_bundle()?;
                    let bootstrap = bundle.vocabulary.build_decoder_bootstrap(1)?;
                    println!(
                        "bundle: model_type={} features_size={} subsampling_factor={} vocab_size={} blank_token_id={:?}",
                        bundle.config.model_type,
                        bundle.config.features_size,
                        bundle.config.subsampling_factor,
                        bundle.vocabulary.len(),
                        bundle.vocabulary.blank_token_id(),
                    );
                    println!(
                        "decoder bootstrap: targets={:?} target_length={:?} state_1_shape={:?} state_2_shape={:?}",
                        bootstrap.targets.shape(),
                        bootstrap.target_length.to_vec(),
                        bootstrap.input_states_1.shape(),
                        bootstrap.input_states_2.shape(),
                    );
                    let mut runtime_manager = manager;
                    let frontend = runtime_manager.run_frontend_dummy(1600)?;
                    let decoder = runtime_manager.run_decoder_dummy_step(1600)?;
                    let greedy_decode = runtime_manager.greedy_decode_dummy(1600, 10)?;
                    println!(
                        "frontend pass: waveform_shape={:?} feature_shape={:?} feature_lengths={:?} encoder_shape={:?} encoder_lengths={:?}",
                        frontend.waveform_shape,
                        frontend.feature_shape,
                        frontend.feature_lengths,
                        frontend.encoder_shape,
                        frontend.encoder_lengths,
                    );
                    println!(
                        "decoder step: logits_shape={:?} prednet_lengths={:?} state_1_shape={:?} state_2_shape={:?}",
                        decoder.logits_shape,
                        decoder.prednet_lengths,
                        decoder.output_state_1_shape,
                        decoder.output_state_2_shape,
                    );
                    println!(
                        "greedy decode: token_ids={:?} timestamps={:?} text={:?}",
                        greedy_decode.token_ids, greedy_decode.timestamps, greedy_decode.text,
                    );
                    for session in runtime_manager.describe() {
                        println!("{} inputs:", session.label);
                        for input in session.inputs {
                            println!("  - {} :: {}", input.name, input.dtype);
                        }
                        println!("{} outputs:", session.label);
                        for output in session.outputs {
                            println!("  - {} :: {}", output.name, output.dtype);
                        }
                    }
                }
            } else {
                println!("model files missing: {}", missing_files.join(", "));
            }
            println!("processed sample: {processed_sample}");
            if cli.verbose {
                println!("config: {config:#?}");
            }
        }
        Command::Run => {
            chirp_rust::recording_overlay::enable_dpi_awareness();
            let app = ChirpApp::new(paths.clone())?;
            app.run()?;
        }
        Command::Listen => {
            run_terminal_session(&cli, &paths)?;
        }
        Command::Record { seconds, wav } => {
            let config = ChirpConfig::load(&paths)?;
            let duration = resolve_recording_duration(seconds, config.max_recording_duration)?;
            let recording = MicrophoneRecorder::record_for(duration)?;
            let processed = transcribe_buffer(
                &paths,
                &config,
                &recording.audio,
                wav.as_deref(),
                cli.verbose,
                Some(&recording.summary),
            )?;

            if cli.verbose {
                println!("record duration_secs={:.2}", duration.as_secs_f32());
            }

            println!("{processed}");
        }
        Command::Transcribe { wav } => {
            let config = ChirpConfig::load(&paths)?;
            let source_audio = AudioBuffer::load_wav(&wav)?;
            let processed =
                transcribe_buffer(&paths, &config, &source_audio, None, cli.verbose, None)?;

            println!("{processed}");
        }
    }

    Ok(())
}

fn run_terminal_session(cli: &Cli, paths: &ProjectPaths) -> Result<()> {
    let config = ChirpConfig::load(paths)?;
    let timeout = if config.max_recording_duration > 0.0 {
        Some(Duration::from_secs_f32(config.max_recording_duration))
    } else {
        None
    };

    println!("Press Enter to start/stop recording. Type q then Enter to quit.");

    let (tx, rx) = mpsc::channel::<String>();
    thread::spawn(move || {
        let stdin = io::stdin();
        for line in stdin.lock().lines() {
            match line {
                Ok(value) => {
                    if tx.send(value).is_err() {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
    });

    let mut active: Option<(ActiveRecording, Instant)> = None;

    loop {
        if let Some((_, started_at)) = &active {
            let remaining = timeout.map(|limit| limit.saturating_sub(started_at.elapsed()));
            match remaining {
                Some(wait) if wait.is_zero() => {
                    let (recording, _) = active.take().expect("recording state present");
                    println!("Maximum recording duration reached.");
                    finish_terminal_recording(recording, &config, paths, cli.verbose)?;
                    println!("Press Enter to start/stop recording. Type q then Enter to quit.");
                }
                Some(wait) => match rx.recv_timeout(wait) {
                    Ok(line) => {
                        if !handle_session_line(line, &mut active, &config, paths, cli.verbose)? {
                            break;
                        }
                    }
                    Err(mpsc::RecvTimeoutError::Timeout) => {
                        let (recording, _) = active.take().expect("recording state present");
                        println!("Maximum recording duration reached.");
                        finish_terminal_recording(recording, &config, paths, cli.verbose)?;
                        println!("Press Enter to start/stop recording. Type q then Enter to quit.");
                    }
                    Err(mpsc::RecvTimeoutError::Disconnected) => break,
                },
                None => match rx.recv() {
                    Ok(line) => {
                        if !handle_session_line(line, &mut active, &config, paths, cli.verbose)? {
                            break;
                        }
                    }
                    Err(_) => break,
                },
            }
        } else {
            match rx.recv() {
                Ok(line) => {
                    if !handle_session_line(line, &mut active, &config, paths, cli.verbose)? {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
    }

    Ok(())
}

fn handle_session_line(
    line: String,
    active: &mut Option<(ActiveRecording, Instant)>,
    config: &ChirpConfig,
    paths: &ProjectPaths,
    verbose: bool,
) -> Result<bool> {
    let trimmed = line.trim();
    if trimmed.eq_ignore_ascii_case("q") {
        if let Some((recording, _)) = active.take() {
            println!("Stopping active recording before exit.");
            finish_terminal_recording(recording, config, paths, verbose)?;
        }
        return Ok(false);
    }

    if active.is_none() {
        let recording = MicrophoneRecorder::start_default()?;
        println!("Recording started.");
        *active = Some((recording, Instant::now()));
    } else {
        let (recording, _) = active.take().expect("recording state present");
        finish_terminal_recording(recording, config, paths, verbose)?;
        println!("Press Enter to start/stop recording. Type q then Enter to quit.");
    }

    Ok(true)
}

fn finish_terminal_recording(
    recording: ActiveRecording,
    config: &ChirpConfig,
    paths: &ProjectPaths,
    verbose: bool,
) -> Result<()> {
    let result = recording.stop()?;
    let processed = transcribe_buffer(
        paths,
        config,
        &result.audio,
        None,
        verbose,
        Some(&result.summary),
    )?;
    if processed.trim().is_empty() {
        println!("Transcription empty.");
    } else {
        println!("{processed}");
    }
    Ok(())
}

fn transcribe_buffer(
    paths: &ProjectPaths,
    config: &ChirpConfig,
    source_audio: &AudioBuffer,
    wav_output: Option<&std::path::Path>,
    verbose: bool,
    capture_summary: Option<&chirp_rust::recording::CaptureSummary>,
) -> Result<String> {
    if source_audio.mono_samples.is_empty() {
        if verbose {
            println!("audio: empty capture; skipping transcription");
            if let Some(output) = wav_output {
                println!("skipped wav save: {}", output.display());
            }
        }
        return Ok(String::new());
    }

    let audio = source_audio.resample_to(16_000)?;

    if let Some(output) = wav_output {
        audio.write_wav(output)?;
    }

    let processor = TextProcessor::new(config.word_overrides.clone(), &config.post_processing);
    let decoded = transcribe_capture(paths, config, &audio, capture_summary, None)?;
    let processed = processor.process(&decoded);

    if verbose {
        if let Some(summary) = capture_summary {
            println!(
                "capture: device={:?} source_rate_hz={} source_channels={} captured_samples={} runtime_samples={}",
                summary.device_name,
                summary.sample_rate_hz,
                summary.channels,
                summary.captured_samples,
                audio.mono_samples.len(),
            );
        } else {
            println!(
                "audio: source_rate_hz={} runtime_rate_hz={} channels={} mono_samples={}",
                source_audio.sample_rate_hz,
                audio.sample_rate_hz,
                source_audio.channels,
                audio.mono_samples.len(),
            );
        }
        if let Some(output) = wav_output {
            println!("saved wav: {}", output.display());
        }
    }

    Ok(processed)
}

fn resolve_recording_duration(requested: Option<f32>, configured_max: f32) -> Result<Duration> {
    let seconds = requested.unwrap_or_else(|| {
        if configured_max > 0.0 {
            configured_max
        } else {
            5.0
        }
    });

    if !seconds.is_finite() || seconds <= 0.0 {
        anyhow::bail!("recording duration must be a positive finite number");
    }

    Ok(Duration::from_secs_f32(seconds))
}

#[cfg(test)]
mod tests {
    use super::resolve_recording_duration;

    #[test]
    fn recording_duration_prefers_requested_value() {
        let duration = resolve_recording_duration(Some(2.5), 45.0).unwrap();
        assert_eq!(duration.as_secs_f32(), 2.5);
    }

    #[test]
    fn recording_duration_falls_back_to_config() {
        let duration = resolve_recording_duration(None, 12.0).unwrap();
        assert_eq!(duration.as_secs_f32(), 12.0);
    }

    #[test]
    fn recording_duration_uses_default_when_config_disabled() {
        let duration = resolve_recording_duration(None, 0.0).unwrap();
        assert_eq!(duration.as_secs_f32(), 5.0);
    }
}

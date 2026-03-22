# Chirp Rust Migration Plan

## Scope

This repository is intended to replace the current Python implementation in `.context/chirp-stt` with a Rust implementation that reaches functional parity.

The reference application currently provides:

- Global hotkey registration to start and stop dictation.
- Microphone capture at 16 kHz mono float32.
- Local Parakeet ONNX speech-to-text inference with lazy reload/unload behavior.
- Processed text injection via direct typing or clipboard paste.
- Configurable word overrides and simple post-processing directives.
- Optional audio start/stop/error feedback.
- Optional always-on-top Windows recording overlay.
- One-time model download/setup flow.
- Dev-mode restart runner.
- Unit tests covering validation, security, lifecycle, and utility behavior.

## Reference Architecture

The Python app is split into a small set of clear subsystems:

- `config_manager.py`: loads and validates `config.toml`, resolves asset/model paths, guards path traversal.
- `main.py`: owns application lifecycle, hotkey toggling, timeout handling, transcription worker scheduling, and smoke-check mode.
- `audio_capture.py`: buffered microphone capture.
- `parakeet_manager.py`: ONNX model load/reload/unload and transcription calls.
- `text_injector.py`: sanitization, word overrides, punctuation normalization, style directives, and keyboard/clipboard injection.
- `audio_feedback.py`: WAV playback with cache and optional volume scaling.
- `recording_overlay.py`: custom Win32 layered overlay window.
- `setup.py`: downloads model assets from Hugging Face.
- `dev.py`: file watcher that restarts the app in development.

That decomposition should be preserved in Rust. The Python codebase is small and cohesive; a direct module-for-module translation is the lowest-risk path.

## Key Migration Constraint

One original project goal is "no new executable required if Python is allowed." A full Rust migration changes that deployment model because a Rust app is normally shipped as a native executable.

Before implementation starts, decide which of these is acceptable:

1. Standard Rust binary distribution.
2. Hybrid architecture where Rust handles performance-critical pieces but Python remains the launcher.
3. Internal-only source build workflow using Cargo.

If full Rust is the goal, the pragmatic assumption is option 1.

## Target Rust Architecture

Recommended crate layout:

- `crates/chirp-cli` or `src/main.rs`: CLI entrypoint and app bootstrap.
- `src/config.rs`: typed config model, TOML parsing, validation, path resolution.
- `src/app.rs`: runtime state machine for idle, recording, transcribing, and error recovery.
- `src/audio/capture.rs`: microphone stream management and sample buffering.
- `src/audio/feedback.rs`: start/stop/error playback and cache.
- `src/stt/parakeet.rs`: ONNX Runtime session management and transcription.
- `src/input/hotkey.rs`: global hotkey registration.
- `src/input/text_injector.rs`: sanitization, style guide, overrides, paste/type injection.
- `src/ui/overlay.rs`: Windows overlay window.
- `src/setup.rs`: model download/install routine.
- `src/dev.rs`: dev watcher/restart runner.

Recommended crates:

- `clap` for CLI.
- `serde` and `toml` for config.
- `thiserror` or `anyhow` for errors.
- `tracing`, `tracing-subscriber`, and optionally `tracing-appender` for logging.
- `cpal` for audio capture.
- `rodio` or `cpal` + WAV decoding for feedback audio.
- `hound` for WAV loading if needed.
- `ort` or direct ONNX Runtime bindings for inference.
- `reqwest` or `hf-hub` for model download.
- `arboard` for clipboard handling.
- `windows` for Win32 APIs, overlay, input simulation, and possibly hotkeys.
- `notify` for dev-mode file watching.
- `regex` for word override matching and punctuation normalization.

## Feature Parity Map

### Must-match behavior

- `config.toml` remains the primary configuration surface.
- Global shortcut toggles recording on and off.
- Captured audio is transcribed on a background worker, not on the hotkey thread.
- Empty transcript is ignored.
- Sanitization strips control characters before injection.
- Word overrides are case-insensitive.
- Post-processing supports:
  - `sentence case`
  - `uppercase`
  - `lowercase`
  - `prepend:`
  - `append:`
- Injection supports both:
  - direct typing
  - clipboard paste using `Ctrl+V` or `Ctrl+Shift+V`
- Clipboard can be cleared after a configurable delay.
- Optional chime playback for start/stop and beep or file for errors.
- Optional top-center recording overlay.
- Model assets are downloaded once and reused.
- Model session can unload after inactivity and reload on demand.
- Max recording duration auto-stops capture.
- Smoke-check mode exists.

### Validation and security parity

Rust implementation must preserve these constraints:

- Reject negative thread counts.
- Reject non-positive clipboard clear delay.
- Reject unsupported injection and paste modes.
- Reject negative model timeout and recording duration.
- Cap maximum recording duration.
- Validate custom sound paths exist.
- Clamp or validate audio feedback volume.
- Guard model path resolution against traversal.
- Strip non-printable characters from injected text.
- Ensure overrides and styling cannot reintroduce unsafe control characters.

## Migration Phases

### Phase 0: Lock down parity targets

- Freeze the Python feature surface and config schema.
- Document exact defaults from `.context/chirp-stt/config.toml`.
- Decide whether the Rust port is Windows-only at first. It should be.
- Decide packaging strategy for model assets and binary release.

Deliverable: approved parity checklist.

### Phase 1: Bootstrap the Rust workspace

- Create a Cargo project with module boundaries matching the reference architecture.
- Add `clippy`, `rustfmt`, and CI test scaffolding.
- Implement typed config parsing and validation first.
- Commit sample `config.toml` and path helpers.

Deliverable: app starts, reads config, and passes config validation tests.

### Phase 2: Build the STT core without UI features

- Implement model directory resolution and setup/download command.
- Integrate ONNX Runtime and load Parakeet from the downloaded model folder.
- Add transcription smoke-check command using a dummy waveform.
- Implement lazy unload/reload timeout behavior.

Deliverable: `chirp setup` and `chirp --check` work end-to-end.

### Phase 3: Add audio capture and app state machine

- Implement microphone capture at 16 kHz mono.
- Build the toggleable recording lifecycle.
- Add max-duration timer handling.
- Push transcription work onto a dedicated worker thread/task.

Deliverable: local recording and transcription work without text injection.

### Phase 4: Add text injection pipeline

- Implement sanitizer, punctuation normalization, style guide parsing, and word overrides.
- Implement Windows typing injection.
- Implement clipboard paste injection and delayed clipboard clearing.
- Add tests specifically for unsafe character stripping and override safety.

Deliverable: transcription can be injected into the focused application.

### Phase 5: Add desktop affordances

- Implement start/stop/error audio playback with caching.
- Implement Windows recording overlay.
- Add graceful degradation when overlay or sound backends are unavailable.

Deliverable: user-visible behavior matches current Python UX.

### Phase 6: Add operational tooling

- Implement `chirp setup`.
- Implement `chirp-dev` restart runner or replace it with a Cargo-native dev command.
- Add structured logging and verbose mode.
- Add packaging scripts and release notes.

Deliverable: developer and operator workflows are in place.

### Phase 7: Expand test coverage to parity

- Port config validation tests.
- Port text injection behavior and security tests.
- Port Parakeet lifecycle tests.
- Port overlay geometry tests.
- Port dev-watcher utility tests.

Deliverable: Rust test suite covers the same behavioral contract as the Python suite.

## Suggested Build Order

Implement in this order:

1. Config and validation.
2. Model setup and inference.
3. App state machine.
4. Audio capture.
5. Text processing and injection.
6. Audio feedback.
7. Overlay.
8. Dev tooling and packaging.

This order keeps the highest technical risk, ONNX integration and Windows input APIs, near the front while avoiding UI work before the core path functions.

## Main Technical Risks

### ONNX Runtime integration

The Python version relies on `onnx-asr` and ONNX Runtime. In Rust, confirm early:

- How Parakeet preprocessing/tokenization is handled.
- Whether the Rust path can call the model directly or needs a wrapper layer.
- Whether the `ort` crate is sufficient or custom runtime bindings are needed.

This is the first spike to run because it determines whether the migration is straightforward or requires a compatibility layer.

### Global hotkeys and text injection

Python currently depends on `keyboard`, which hides substantial platform-specific behavior. In Rust, these pieces will likely require direct Win32 APIs:

- `RegisterHotKey` or a low-level hook strategy.
- `SendInput` for typing and paste shortcuts.
- Clipboard access through Win32 or a crate like `arboard`.

These should be implemented Windows-first and tested on a real machine, not only in unit tests.

### Overlay rendering

The overlay is hand-built using Win32 layered windows and GDI+. Rust should keep this Windows-native rather than introducing a heavy GUI framework. The `windows` crate is the right default.

### Audio stack differences

`sounddevice` in Python abstracts microphone and playback well. In Rust, capture and playback may need separate implementations or crates. Expect more edge-case handling around device availability and sample conversion.

## Recommended First Milestone

The first real milestone should be:

- `chirp setup` downloads the model.
- `chirp --check` loads the model and returns a processed sample transcript.
- `config.toml` parsing and validation are complete.

Do not start with the overlay or hotkey system. Prove the model and config path first.

## Immediate Next Steps

1. Initialize the Rust crate and commit the module skeleton.
2. Recreate the config schema and validation rules from the Python app.
3. Run a dedicated ONNX/Parakeet spike to verify Rust inference is viable.
4. After the inference spike passes, build the app state machine around it.

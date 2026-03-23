# Release Notes

## v0.1.0

Initial public ChirpR release.

### Runtime

- Added a Windows-native dictation app around local Parakeet ONNX transcription.
- Added global hotkey recording with configurable max-duration auto-stop.
- Added paste and type injection modes, with paste as the default for reliability.
- Preserved prior clipboard text after paste injection when possible.
- Added start, stop, and error audio feedback.
- Added a Windows recording overlay with DPI-aware layout.
- Added idle model unload plus background model prewarm during recording.

### Tooling

- Added `chirpr-cli dev` for automatic restart during development.
- Added a Windows release staging script at `scripts/build-release.ps1`.
- Added a portable `run-portable.cmd` entrypoint with automatic setup on first launch.
- Added a per-user Windows MSI installer plus an `install.cmd` wrapper for install directory, autostart, and launch options.

### Configuration

- Default hotkey is `ctrl+shift+space`.
- `config.toml` includes inline comments describing each supported setting.
- Included default filler-word removals for `um` and `uh`.

### Windows Integration

- Added `autostart enable|disable|status` support using the current-user Run registry key.
- Added Start menu shortcuts for launching, settings, and uninstall.

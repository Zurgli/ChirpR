# Release Notes

## Unreleased

### Runtime

- Implemented a Windows-native dictation app around Parakeet ONNX transcription.
- Added global hotkey recording with configurable max-duration auto-stop.
- Added paste and type injection modes, with paste now the default for reliability.
- Preserved prior clipboard text after paste injection when possible.
- Added start, stop, and error audio feedback.
- Added a Windows recording overlay with antialiased drawing and DPI-aware layout.

### Tooling

- Added `chirpr-cli dev` for automatic restart during development.
- Added a Windows release staging script at `scripts/build-release.ps1`.
- Added a portable `run-portable.cmd` entrypoint with automatic setup on first launch.
- Added a proper per-user Windows MSI installer plus an `install.cmd` wrapper for install directory, autostart, and launch options.

### Configuration

- Default hotkey is `ctrl+shift+space`.
- `config.toml` includes inline comments describing each supported setting.

### Windows integration

- Added `autostart enable|disable|status` support using the current-user Run registry key.

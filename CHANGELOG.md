# Changelog

All notable changes to this project will be documented in this file.

## [0.2.0] - 2026-03-23

### Added
- NSIS-based Windows installer replacing WiX/MSI
- Automatic kill of running instance before install/upgrade
- PowerShell-based uninstaller script

### Changed
- Updated README to reflect NSIS installer

## [0.2.1] - 2026-03-31

### Changed
- Prevent model unload while recording or transcribing
- Reset model idle timeout after transcription completes
- Relaunching `chirpr.exe` now replaces older running ChirpR instances from the same install

## [0.1.0] - 2025-01-19

### Added
- Initial release
- Local speech-to-text with Parakeet ONNX models
- Global hotkey dictation with `Ctrl+Shift+Space`
- Paste and type injection modes
- Recording overlay with audio feedback
- MSI-based Windows installer
- Optional run-at-login support

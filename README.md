# ChirpR

ChirpR is a Windows-native Rust dictation app built around local Parakeet ONNX transcription.

It is a Rust implementation of the original upstream `Whamp/chirp` workflow and desktop UX, adapted for a Windows-first packaged experience.

## Features

- Local speech-to-text with Parakeet ONNX models
- Global hotkey dictation with `Ctrl+Shift+Space`
- Paste injection by default for better Windows app compatibility
- Optional typed injection with `injection_mode = "type"`
- Recording overlay with audio start/stop/error feedback
- Idle model unload and background model prewarm
- NSIS-based Windows installer
- Optional current-user run-at-login support

## Quick Start

Portable bundle:

```powershell
.\run-portable.cmd
```

Installed bundle (NSIS installer):

```powershell
.\ChirpRSetup.exe
```

After install or launch, use `Ctrl+Shift+Space` to start dictation.

## Development

Run from the repo root:

```powershell
cargo run -- check
cargo run -- setup
cargo run -- run
cargo run -- dev
```

Forward app args through the dev runner with a second `--`:

```powershell
cargo run -- dev -- --verbose
```

## Release Build

Build and stage a Windows release bundle:

```powershell
.\scripts\build-release.ps1
```

This produces:

- `dist\chirpr-windows-x64\chirpr-cli.exe`
- `dist\chirpr-windows-x64\chirpr.exe`
- `dist\chirpr-windows-x64\config.toml`
- `dist\chirpr-windows-x64\LICENSE`
- `dist\chirpr-windows-x64\assets\sounds\`
- `dist\chirpr-windows-x64\run-portable.cmd`
- `dist\chirpr-windows-x64\ChirpRSetup.exe`
- `dist\chirpr-windows-x64\install.ps1`
- `dist\chirpr-windows-x64\Uninstall.ps1`

Portable launch runs setup on first use and downloads model files if they are missing.

The NSIS installer flow supports:

- install directory selection
- automatically kills running instance before upgrade
- launches app after installation completes

The installed app creates Start menu shortcuts for:

- `ChirpR`
- `Uninstall ChirpR`

## Logging And Models

Runtime logs currently go to `stderr`.

Model files are stored under:

- repo/dev use: `assets\models\`
- portable bundle: `.\assets\models\`
- installed bundle: inside the MSI-installed app directory under `assets\models\`

## License

ChirpR is open source. See [LICENSE](/E:/development/chirp/chirp-rust/LICENSE).

# Chirp Rust

Chirp Rust is the Windows-native Rust port of the original Chirp dictation app.

The original Python implementation came from the upstream `Whamp/chirp` project. This repository is the Rust-based implementation of that app's behavior and workflow.

## Current status

- Local Parakeet transcription is working.
- Global hotkey dictation flow is working.
- Paste injection is the default because it is more reliable across Windows apps.
- Typing injection remains available through `injection_mode = "type"` in `config.toml`.
- Audio feedback, recording overlay, setup flow, and dev runner are implemented.

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

## Release build

Build and stage a Windows release bundle:

```powershell
.\scripts\build-release.ps1
```

That produces:

- `dist\chirp-rust-windows-x64\chirp-rust.exe`
- `dist\chirp-rust-windows-x64\config.toml`
- `dist\chirp-rust-windows-x64\assets\sounds\`
- `dist\chirp-rust-windows-x64\run-portable.cmd`
- `dist\chirp-rust-windows-x64\ChirpRSetup.msi`
- `dist\chirp-rust-windows-x64\install.cmd`

Portable launch downloads models on first run if they are missing.
The MSI bundles the configured int8 Parakeet model so installed use does not need a separate setup step.

Portable use:

```powershell
.\run-portable.cmd
```

Installed use:

```powershell
.\install.cmd
```

You can also open `.\ChirpRSetup.msi` directly.

The installer flow prompts for:

- install directory
- whether to enable Windows login startup
- whether to launch immediately after install

The installed app also creates Start menu shortcuts for:

- `ChirpR`
- `ChirpR Settings`
- `Uninstall ChirpR`

Uninstall the MSI-installed app from Windows Settings > Installed apps.

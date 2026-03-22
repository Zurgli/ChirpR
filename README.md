# Chirp Rust

Chirp Rust is the Windows-native Rust port of the original Chirp local dictation app in `.context/chirp-stt`.

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

Models are not bundled. After staging or copying the bundle to another machine, run:

```powershell
.\chirp-rust.exe setup
.\chirp-rust.exe run
```

# Release Checklist

## Before Tagging

- Run `cargo test`
- Run `cargo run -- check`
- Build the release bundle with `.\scripts\build-release.ps1`
- Confirm `dist\chirpr-windows-x64\ChirpRSetup.msi` exists
- Confirm `dist\chirpr-windows-x64\run-portable.cmd` works on a clean machine or VM
- Confirm `install.cmd` and direct `ChirpRSetup.msi` install both succeed
- Confirm Start menu shortcuts are created after install
- Confirm uninstall removes the app cleanly enough for the current release

## Release Artifacts

- `dist\chirpr-windows-x64\ChirpRSetup.msi`
- `dist\chirpr-windows-x64\chirpr.exe`
- `dist\chirpr-windows-x64\chirpr-cli.exe`
- `dist\chirpr-windows-x64\run-portable.cmd`
- `dist\chirpr-windows-x64\config.toml`
- `dist\chirpr-windows-x64\LICENSE`

## Publish

- Update [RELEASE_NOTES.md](/E:/development/chirp/chirp-rust/RELEASE_NOTES.md)
- Commit the final release docs or fixes
- Create and push the release tag
- Draft the GitHub release from the tag
- Upload the release bundle or link the built artifacts

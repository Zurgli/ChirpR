# Release Notes

## v0.2.3

This release fixes installed Windows packaging behavior and removes the deprecated non-NSIS installer paths.

### Fixed

- Start Menu launches now resolve `config.toml` and `assets` from the installed executable location instead of the shortcut working directory.
- NSIS installs now enable current-user autostart by default.
- NSIS uninstall now removes the current-user Run entry that was added during install.

### Changed

- Packaged releases are now NSIS-only.
- Removed the old WiX/MSI and PowerShell installer paths from the repository and release flow.

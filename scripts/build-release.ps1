[CmdletBinding()]
param(
    [string]$TargetTriple = "x86_64-pc-windows-msvc",
    [string]$OutputRoot = "dist",
    [switch]$SkipTests
)

$ErrorActionPreference = "Stop"

$repoRoot = Split-Path -Parent $PSScriptRoot
$releaseRoot = Join-Path $repoRoot $OutputRoot
$bundleRoot = Join-Path $releaseRoot "chirp-rust-windows-x64"
$assetsSource = Join-Path $repoRoot "assets"
$soundsSource = Join-Path $assetsSource "sounds"
$binarySource = Join-Path $repoRoot "target\$TargetTriple\release\chirp-rust.exe"

Write-Host "Preparing release bundle in $bundleRoot"

if (-not $SkipTests) {
    Write-Host "Running test suite"
    cargo test --target $TargetTriple
}

Write-Host "Building release binary"
cargo build --release --target $TargetTriple

if (-not (Test-Path $binarySource)) {
    throw "release binary not found at $binarySource"
}

Remove-Item -Recurse -Force $bundleRoot -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Path $bundleRoot | Out-Null
New-Item -ItemType Directory -Path (Join-Path $bundleRoot "assets") | Out-Null

Copy-Item $binarySource (Join-Path $bundleRoot "chirp-rust.exe")
Copy-Item (Join-Path $repoRoot "config.toml") (Join-Path $bundleRoot "config.toml")

if (Test-Path $soundsSource) {
    Copy-Item $soundsSource (Join-Path $bundleRoot "assets") -Recurse
}

@"
@echo off
pushd "%~dp0"
".\chirp-rust.exe" run
popd
"@ | Set-Content (Join-Path $bundleRoot "run-portable.cmd")

@"
@echo off
powershell -ExecutionPolicy Bypass -File "%~dp0install.ps1"
"@ | Set-Content (Join-Path $bundleRoot "install.cmd")

@"
@echo off
powershell -ExecutionPolicy Bypass -File "%~dp0uninstall.ps1"
"@ | Set-Content (Join-Path $bundleRoot "uninstall.cmd")

@'
$ErrorActionPreference = "Stop"

$bundleRoot = Split-Path -Parent $MyInvocation.MyCommand.Path
$defaultAppRoot = Join-Path $env:LOCALAPPDATA "ChirpRust"

Write-Host ""
Write-Host "Chirp Rust Installer"
Write-Host "--------------------"
Write-Host "Portable use is always available via run-portable.cmd from this folder."
Write-Host ""

$installChoice = $Host.UI.PromptForChoice(
    "Install mode",
    "Choose how you want to use Chirp Rust.",
    [System.Collections.ObjectModel.Collection[System.Management.Automation.Host.ChoiceDescription]]@(
        (New-Object System.Management.Automation.Host.ChoiceDescription "&Portable", "Do not install. Keep running from this folder."),
        (New-Object System.Management.Automation.Host.ChoiceDescription "&Install", "Copy Chirp Rust into a user app folder.")
    ),
    1
)

if ($installChoice -eq 0) {
    Write-Host ""
    Write-Host "Portable mode selected."
    Write-Host "Run run-portable.cmd from this folder whenever you want to start Chirp."
    exit 0
}

$enteredRoot = Read-Host "Install directory [default: $defaultAppRoot]"
$appRoot = if ([string]::IsNullOrWhiteSpace($enteredRoot)) { $defaultAppRoot } else { $enteredRoot.Trim() }

$autostartChoice = $Host.UI.PromptForChoice(
    "Login startup",
    "Enable Chirp Rust automatically when you sign in?",
    [System.Collections.ObjectModel.Collection[System.Management.Automation.Host.ChoiceDescription]]@(
        (New-Object System.Management.Automation.Host.ChoiceDescription "&No", "Do not enable login startup."),
        (New-Object System.Management.Automation.Host.ChoiceDescription "&Yes", "Enable login startup for the current user.")
    ),
    0
)

$launchChoice = $Host.UI.PromptForChoice(
    "Launch now",
    "Start Chirp Rust immediately after install?",
    [System.Collections.ObjectModel.Collection[System.Management.Automation.Host.ChoiceDescription]]@(
        (New-Object System.Management.Automation.Host.ChoiceDescription "&Yes", "Launch Chirp when setup finishes."),
        (New-Object System.Management.Automation.Host.ChoiceDescription "&No", "Do not launch now.")
    ),
    0
)

Write-Host ""
Write-Host "Installing Chirp Rust to $appRoot"
New-Item -ItemType Directory -Path $appRoot -Force | Out-Null
Copy-Item (Join-Path $bundleRoot "*") $appRoot -Recurse -Force

Push-Location $appRoot
try {
    & ".\chirp-rust.exe" setup
    if ($autostartChoice -eq 1) {
        & ".\chirp-rust.exe" autostart enable
    }
    else {
        & ".\chirp-rust.exe" autostart disable
    }
    if ($launchChoice -eq 0) {
        Start-Process ".\chirp-rust.exe" -ArgumentList "run"
    }
}
finally {
    Pop-Location
}

Write-Host ""
Write-Host "Chirp Rust installed to $appRoot"
'@ | Set-Content (Join-Path $bundleRoot "install.ps1")

@'
$ErrorActionPreference = "Stop"

$appRoot = Join-Path $env:LOCALAPPDATA "ChirpRust"

if (Test-Path (Join-Path $appRoot "chirp-rust.exe")) {
    Push-Location $appRoot
    try {
        & ".\chirp-rust.exe" autostart disable
    }
    finally {
        Pop-Location
    }
}

Get-Process | Where-Object { $_.Path -like "$appRoot*" } | Stop-Process -Force -ErrorAction SilentlyContinue
Remove-Item -Recurse -Force $appRoot -ErrorAction SilentlyContinue

Write-Host "Chirp Rust removed from $appRoot"
'@ | Set-Content (Join-Path $bundleRoot "uninstall.ps1")

@"
Chirp Rust release bundle

Contents:
- chirp-rust.exe
- config.toml
- assets\sounds\
- run-portable.cmd
- install.cmd / install.ps1
- uninstall.cmd / uninstall.ps1

Usage:
1. Portable: run .\run-portable.cmd from this folder
2. Installed: run .\install.cmd and choose your install options

Notes:
- Models are not bundled. The setup command downloads them into assets\models.
- Paste injection is the default because it is more reliable across Windows apps.
"@ | Set-Content (Join-Path $bundleRoot "README.txt")

Write-Host "Release bundle ready:"
Write-Host "  $bundleRoot"

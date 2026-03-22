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
Chirp Rust release bundle

Contents:
- chirp-rust.exe
- config.toml
- assets\sounds\

First run:
1. Run .\chirp-rust.exe setup
2. Run .\chirp-rust.exe run

Notes:
- Models are not bundled. The setup command downloads them into assets\models.
- Paste injection is the default because it is more reliable across Windows apps.
"@ | Set-Content (Join-Path $bundleRoot "README.txt")

Write-Host "Release bundle ready:"
Write-Host "  $bundleRoot"

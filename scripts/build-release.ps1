[CmdletBinding()]
param(
    [string]$TargetTriple = "x86_64-pc-windows-msvc",
    [string]$OutputRoot = "dist",
    [switch]$SkipTests,
    [switch]$SkipBuild
)

$ErrorActionPreference = "Stop"

$repoRoot = Split-Path -Parent $PSScriptRoot
$releaseRoot = Join-Path $repoRoot $OutputRoot
$bundleRoot = Join-Path $releaseRoot "chirpr-windows-x64"
$assetsSource = Join-Path $repoRoot "assets"
$soundsSource = Join-Path $assetsSource "sounds"
$binarySource = Join-Path $repoRoot "target\$TargetTriple\release\chirpr-cli.exe"
$launcherSource = Join-Path $repoRoot "target\$TargetTriple\release\chirpr.exe"
$cargoToml = Join-Path $repoRoot "Cargo.toml"
$nsiScript = Join-Path $repoRoot "installer\ChirpRSetup.nsi"
$installerOutput = Join-Path $repoRoot "installer\ChirpRSetup.exe"

$cargoVersion = (Select-String -Path $cargoToml -Pattern '^version\s*=\s*"([^"]+)"').Matches.Groups[1].Value
if ([string]::IsNullOrWhiteSpace($cargoVersion)) {
    throw "failed to read package version from $cargoToml"
}

$makensisPath = $null
$nsisPaths = @(
    "${env:ProgramFiles(x86)}\NSIS\makensis.exe",
    "${env:ProgramFiles}\NSIS\makensis.exe"
)
foreach ($path in $nsisPaths) {
    if (Test-Path $path) {
        $makensisPath = $path
        break
    }
}

if (-not $makensisPath) {
    throw "NSIS not found. Please install from https://nsis.sourceforge.io/ or run: winget install NSIS.NSIS"
}

Write-Host "NSIS found at: $makensisPath"

Write-Host "Preparing release bundle in $bundleRoot"

if (-not $SkipTests) {
    Write-Host "Running test suite"
    cargo test --target $TargetTriple
}

if (-not $SkipBuild) {
    Write-Host "Building release binary"
    cargo build --release --target $TargetTriple
}

if (-not (Test-Path $binarySource)) {
    throw "release binary not found at $binarySource"
}

if (-not (Test-Path $launcherSource)) {
    throw "release launcher not found at $launcherSource"
}

$requiredFiles = @(
    $binarySource,
    $launcherSource,
    $soundsSource,
    $nsiScript
)
foreach ($path in $requiredFiles) {
    if (-not (Test-Path $path)) {
        throw "required file missing: $path"
    }
}

if (Test-Path $bundleRoot) {
    Write-Host "Cleaning existing bundle..."
    Remove-Item -Recurse -Force $bundleRoot
}

Write-Host "Creating bundle directory structure..."
New-Item -ItemType Directory -Path $bundleRoot -Force | Out-Null
New-Item -ItemType Directory -Path (Join-Path $bundleRoot "assets\sounds") -Force | Out-Null

Write-Host "Copying binaries..."
Copy-Item $binarySource $bundleRoot
Copy-Item $launcherSource $bundleRoot

Write-Host "Copying configuration..."
$configSource = Join-Path $repoRoot "config.toml"
if (Test-Path $configSource) {
    Copy-Item $configSource $bundleRoot
}

Write-Host "Copying sounds..."
if (Test-Path $soundsSource) {
    Copy-Item "$soundsSource\*" (Join-Path $bundleRoot "assets\sounds") -Force
}

Write-Host "Copying license..."
$licenseSource = Join-Path $repoRoot "LICENSE"
if (Test-Path $licenseSource) {
    Copy-Item $licenseSource $bundleRoot
}

Write-Host "Building NSIS installer..."

$nsiContent = Get-Content $nsiScript -Raw
$nsiContent = $nsiContent -replace '\$\{BUILD_ROOT\}', $repoRoot
$tempNsi = Join-Path $repoRoot "ChirpRSetup_temp.nsi"
Set-Content -Path $tempNsi -Value $nsiContent -NoNewline

Push-Location $repoRoot
& $makensisPath $tempNsi
if ($LASTEXITCODE -ne 0) {
    Pop-Location
    Remove-Item $tempNsi -Force -EA SilentlyContinue
    throw "NSIS build failed"
}
Pop-Location

Remove-Item $tempNsi -Force -EA SilentlyContinue

$builtInstaller = Join-Path $repoRoot "ChirpRSetup.exe"
if (Test-Path $builtInstaller) {
    if (-not (Test-Path (Split-Path $installerOutput))) {
        New-Item -ItemType Directory -Path (Split-Path $installerOutput) -Force | Out-Null
    }
    Move-Item $builtInstaller $installerOutput -Force
}

if (Test-Path $installerOutput) {
    Write-Host "Copying installer to bundle..."
    Copy-Item $installerOutput $bundleRoot -Force
}

Write-Host ""
Write-Host "Release bundle ready: $bundleRoot" -ForegroundColor Green
Write-Host "Contents:"
Get-ChildItem $bundleRoot | ForEach-Object { Write-Host "  $($_.Name)" }

$installerInBundle = Join-Path $bundleRoot "ChirpRSetup.exe"
if (Test-Path $installerInBundle) {
    $installerSize = [math]::Round((Get-Item $installerInBundle).Length / 1MB, 1)
    Write-Host ""
    Write-Host "Installer: $installerSize MB" -ForegroundColor Cyan
}

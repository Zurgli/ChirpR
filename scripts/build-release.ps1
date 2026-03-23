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
$modelsSource = Join-Path $assetsSource "models\nemo-parakeet-tdt-0.6b-v3-int8"
$binarySource = Join-Path $repoRoot "target\$TargetTriple\release\chirp-rust.exe"
$launcherSource = Join-Path $repoRoot "target\$TargetTriple\release\chirpr.exe"
$cargoToml = Join-Path $repoRoot "Cargo.toml"
$installerProject = Join-Path $repoRoot "installer\ChirpRust.Installer.wixproj"
$installerOutput = Join-Path $repoRoot "installer\bin\x64\Release\ChirpRSetup.msi"
$licenseSource = Join-Path $repoRoot "installer\License.rtf"
$uninstallCmdSource = Join-Path $repoRoot "installer\Uninstall.cmd"
$uninstallScriptSource = Join-Path $repoRoot "installer\Uninstall.ps1"

$cargoVersion = (Select-String -Path $cargoToml -Pattern '^version\s*=\s*"([^"]+)"').Matches.Groups[1].Value
if ([string]::IsNullOrWhiteSpace($cargoVersion)) {
    throw "failed to read package version from $cargoToml"
}

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

if (-not (Test-Path $launcherSource)) {
    throw "release launcher not found at $launcherSource"
}

$requiredInstallerFiles = @(
    $installerProject,
    $binarySource,
    $launcherSource,
    (Join-Path $repoRoot "config.toml"),
    (Join-Path $repoRoot "LICENSE"),
    $licenseSource,
    $uninstallCmdSource,
    $uninstallScriptSource,
    (Join-Path $soundsSource "ping-up.wav"),
    (Join-Path $soundsSource "ping-down.wav"),
    (Join-Path $modelsSource "config.json"),
    (Join-Path $modelsSource "decoder_joint-model.int8.onnx"),
    (Join-Path $modelsSource "encoder-model.int8.onnx"),
    (Join-Path $modelsSource "nemo128.onnx"),
    (Join-Path $modelsSource "vocab.txt")
)

foreach ($path in $requiredInstallerFiles) {
    if (-not (Test-Path $path)) {
        throw "required installer input missing: $path"
    }
}

Write-Host "Building MSI installer"
dotnet build $installerProject `
    -c Release `
    -p:ProductVersion=$cargoVersion `
    -p:BinarySource=$binarySource `
    -p:LauncherSource=$launcherSource `
    -p:ConfigSource=$(Join-Path $repoRoot "config.toml") `
    -p:LicenseSource=$licenseSource `
    -p:UninstallCmdSource=$uninstallCmdSource `
    -p:UninstallScriptSource=$uninstallScriptSource `
    -p:PingUpSource=$(Join-Path $soundsSource "ping-up.wav") `
    -p:PingDownSource=$(Join-Path $soundsSource "ping-down.wav") `
    -p:ModelConfigSource=$(Join-Path $modelsSource "config.json") `
    -p:ModelDecoderSource=$(Join-Path $modelsSource "decoder_joint-model.int8.onnx") `
    -p:ModelEncoderSource=$(Join-Path $modelsSource "encoder-model.int8.onnx") `
    -p:ModelFeatureSource=$(Join-Path $modelsSource "nemo128.onnx") `
    -p:ModelVocabSource=$(Join-Path $modelsSource "vocab.txt")

if (-not (Test-Path $installerOutput)) {
    throw "MSI build did not produce $installerOutput"
}

Remove-Item -Recurse -Force $bundleRoot -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Path $bundleRoot | Out-Null
New-Item -ItemType Directory -Path (Join-Path $bundleRoot "assets") | Out-Null

Copy-Item $binarySource (Join-Path $bundleRoot "chirp-rust.exe")
Copy-Item $launcherSource (Join-Path $bundleRoot "chirpr.exe")
Copy-Item (Join-Path $repoRoot "config.toml") (Join-Path $bundleRoot "config.toml")
Copy-Item (Join-Path $repoRoot "LICENSE") (Join-Path $bundleRoot "LICENSE")
Copy-Item $uninstallCmdSource (Join-Path $bundleRoot "uninstall.cmd")
Copy-Item $uninstallScriptSource (Join-Path $bundleRoot "uninstall.ps1")
Copy-Item $installerOutput (Join-Path $bundleRoot "ChirpRSetup.msi")

if (Test-Path $soundsSource) {
    Copy-Item $soundsSource (Join-Path $bundleRoot "assets") -Recurse
}

@"
@echo off
pushd "%~dp0"
".\chirp-rust.exe" setup
if errorlevel 1 (
  popd
  exit /b %errorlevel%
)
".\chirp-rust.exe" run
popd
"@ | Set-Content (Join-Path $bundleRoot "run-portable.cmd")

@"
@echo off
powershell -ExecutionPolicy Bypass -File "%~dp0install.ps1"
"@ | Set-Content (Join-Path $bundleRoot "install.cmd")

@'
$ErrorActionPreference = "Stop"

$bundleRoot = Split-Path -Parent $MyInvocation.MyCommand.Path
$msiPath = Join-Path $bundleRoot "ChirpRSetup.msi"
$defaultAppRoot = Join-Path $env:LOCALAPPDATA "Programs\ChirpR"
$stateKey = "HKCU:\Software\ChirpRust"

if (Test-Path $stateKey) {
    $state = Get-ItemProperty -Path $stateKey -ErrorAction SilentlyContinue
    if ($state -and -not [string]::IsNullOrWhiteSpace($state.InstallRoot)) {
        $defaultAppRoot = $state.InstallRoot
    }
}

if (-not (Test-Path $msiPath)) {
    throw "installer package not found at $msiPath"
}

Write-Host ""
Write-Host "ChirpR Installer"
Write-Host "--------------------"
Write-Host "Portable use is always available via run-portable.cmd from this folder."
Write-Host ""

$enteredRoot = Read-Host "Install directory [default: $defaultAppRoot]"
$appRoot = if ([string]::IsNullOrWhiteSpace($enteredRoot)) { $defaultAppRoot } else { $enteredRoot.Trim() }

$autostartChoice = $Host.UI.PromptForChoice(
    "Login startup",
    "Enable ChirpR automatically when you sign in?",
    [System.Collections.ObjectModel.Collection[System.Management.Automation.Host.ChoiceDescription]]@(
        (New-Object System.Management.Automation.Host.ChoiceDescription "&No", "Do not enable login startup."),
        (New-Object System.Management.Automation.Host.ChoiceDescription "&Yes", "Enable login startup for the current user.")
    ),
    0
)

$launchChoice = $Host.UI.PromptForChoice(
    "Launch now",
    "Start ChirpR immediately after install?",
    [System.Collections.ObjectModel.Collection[System.Management.Automation.Host.ChoiceDescription]]@(
        (New-Object System.Management.Automation.Host.ChoiceDescription "&Yes", "Launch Chirp when setup finishes."),
        (New-Object System.Management.Automation.Host.ChoiceDescription "&No", "Do not launch now.")
    ),
    0
)

$autostartValue = if ($autostartChoice -eq 1) { "1" } else { "0" }
$logPath = Join-Path $env:TEMP "chirpr-msi-install.log"

Write-Host ""
Write-Host "Installing ChirpR to $appRoot"
$arguments = @(
    "/i",
    $msiPath,
    "INSTALLFOLDER=$appRoot",
    "AUTOSTART=$autostartValue",
    "/L*V",
    $logPath
)

$process = Start-Process -FilePath "msiexec.exe" -ArgumentList $arguments -Wait -PassThru
if ($process.ExitCode -ne 0) {
    Add-Type -AssemblyName PresentationFramework
    [System.Windows.MessageBox]::Show(
        "ChirpR install failed.`n`nExit code: $($process.ExitCode)`nLog: $logPath",
        "ChirpR Installer",
        [System.Windows.MessageBoxButton]::OK,
        [System.Windows.MessageBoxImage]::Error
    ) | Out-Null
    throw "msiexec failed with exit code $($process.ExitCode). log: $logPath"
}

$didLaunch = $false
if ($launchChoice -eq 0) {
    $launchCommand = "Set-Location -LiteralPath '$($appRoot.Replace("'", "''"))'; & '.\chirpr.exe'"
    Start-Process -FilePath "powershell.exe" -ArgumentList @(
        "-NoProfile",
        "-WindowStyle",
        "Hidden",
        "-ExecutionPolicy",
        "Bypass",
        "-Command",
        $launchCommand
    ) -WorkingDirectory $appRoot | Out-Null
    $didLaunch = $true
}

Add-Type -AssemblyName PresentationFramework

Write-Host ""
Write-Host "ChirpR installed to $appRoot"
if ($didLaunch) {
    Write-Host "ChirpR is now running in the background."
}
else {
    Write-Host "ChirpR is installed but not running yet."
}
Write-Host "Use Ctrl+Shift+Space to start dictation."
Write-Host ""
Write-Host "Start menu shortcuts:"
Write-Host "- ChirpR"
Write-Host "- ChirpR Settings"
Write-Host "- Uninstall ChirpR"
Write-Host "Uninstall it later from Windows Settings > Installed apps."

$statusLine = if ($didLaunch) {
    "ChirpR is now running in the background."
}
else {
    "ChirpR is installed but not running yet."
}

$message = @"
ChirpR installed successfully.

$statusLine
Use Ctrl+Shift+Space to start dictation.

Start menu shortcuts:
- ChirpR
- ChirpR Settings
- Uninstall ChirpR

Install location:
$appRoot
"@

[System.Windows.MessageBox]::Show(
    $message,
    "ChirpR Installer",
    [System.Windows.MessageBoxButton]::OK,
    [System.Windows.MessageBoxImage]::Information
) | Out-Null
'@ | Set-Content (Join-Path $bundleRoot "install.ps1")

@"
ChirpR release bundle

Contents:
- chirp-rust.exe
- chirpr.exe
- config.toml
- LICENSE
- uninstall.cmd / uninstall.ps1
- assets\sounds\
- run-portable.cmd
- ChirpRSetup.msi
- install.cmd / install.ps1

Usage:
1. Portable: run .\run-portable.cmd from this folder
2. Installed: run .\install.cmd or open ChirpRSetup.msi

Notes:
- Portable launch auto-runs setup and downloads any missing model files into assets\models.
- The MSI includes the configured int8 Parakeet model bundle so installed use does not need a separate setup step.
- Paste injection is the default because it is more reliable across Windows apps.
"@ | Set-Content (Join-Path $bundleRoot "README.txt")

Write-Host "Release bundle ready:"
Write-Host "  $bundleRoot"

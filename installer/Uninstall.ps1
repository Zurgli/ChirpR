$ErrorActionPreference = "Stop"

$installRoot = Split-Path -Parent $MyInvocation.MyCommand.Path
$stateKey = "HKCU:\Software\ChirpRust"
$productCode = $null

function Normalize-InstallPath([string]$path) {
    if ([string]::IsNullOrWhiteSpace($path)) {
        return ""
    }
    return ($path.Trim().TrimEnd('\')).ToLowerInvariant()
}

$normalizedInstallRoot = Normalize-InstallPath $installRoot

if (Test-Path $stateKey) {
    $state = Get-ItemProperty -Path $stateKey -ErrorAction SilentlyContinue
    if ($state -and -not [string]::IsNullOrWhiteSpace($state.ProductCode)) {
        $productCode = $state.ProductCode
    }
    elseif ($state -and (Normalize-InstallPath $state.InstallRoot) -eq $normalizedInstallRoot) {
        $productCode = $state.ProductCode
    }
}

if ([string]::IsNullOrWhiteSpace($productCode)) {
    $arpMatch = Get-ItemProperty HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\* -ErrorAction SilentlyContinue |
        Where-Object {
            $_.DisplayName -eq "ChirpR" -and
            (Normalize-InstallPath $_.InstallLocation) -eq $normalizedInstallRoot
        } |
        Select-Object -First 1
    if ($arpMatch) {
        if ($arpMatch.PSChildName -match '^\{.+\}$') {
            $productCode = $arpMatch.PSChildName
        }
        elseif ($arpMatch.UninstallString -match '\{[A-F0-9\-]+\}') {
            $productCode = $Matches[0]
        }
    }
}

if ([string]::IsNullOrWhiteSpace($productCode)) {
    Write-Error "Could not determine the installed ChirpR product code for $installRoot"
    exit 1
}

Start-Process -FilePath "msiexec.exe" -ArgumentList @("/x", $productCode) -Wait

Get-Process -ErrorAction SilentlyContinue |
    Where-Object {
        $_.Path -and
        (Normalize-InstallPath (Split-Path -Parent $_.Path)) -eq $normalizedInstallRoot
    } |
    Stop-Process -Force -ErrorAction SilentlyContinue

$cleanupScript = Join-Path $env:TEMP ("chirpr-cleanup-" + [guid]::NewGuid().ToString("N") + ".ps1")
$cleanupContent = @"
Start-Sleep -Seconds 2
`$installRoot = '$($installRoot.Replace("'", "''"))'
`$stateKey = '$($stateKey.Replace("'", "''"))'

Get-Process -ErrorAction SilentlyContinue |
    Where-Object {
        `$_.Path -and
        ((Split-Path -Parent `$_.Path).Trim().TrimEnd('\').ToLowerInvariant()) -eq `$installRoot.Trim().TrimEnd('\').ToLowerInvariant()
    } |
    Stop-Process -Force -ErrorAction SilentlyContinue

if (Test-Path `$stateKey) {
    Remove-Item `$stateKey -Recurse -Force -ErrorAction SilentlyContinue
}

Remove-Item `$installRoot -Recurse -Force -ErrorAction SilentlyContinue
Remove-Item '$($cleanupScript.Replace("'", "''"))' -Force -ErrorAction SilentlyContinue
"@

Set-Content -Path $cleanupScript -Value $cleanupContent -Encoding UTF8
Start-Process -FilePath "powershell.exe" -ArgumentList @(
    "-NoProfile",
    "-ExecutionPolicy",
    "Bypass",
    "-File",
    $cleanupScript
) -WindowStyle Hidden | Out-Null

# AI Handoff — Windows PowerShell installer.
#
# One-line install (downloads and runs this script):
#
#   Set-ExecutionPolicy Bypass -Scope Process -Force; irm https://raw.githubusercontent.com/Lumisia/aho__ai-handoff/master/scripts/install.ps1 | iex
#
# To pass options, fetch the script into a scriptblock instead:
#
#   & ([scriptblock]::Create((irm https://raw.githubusercontent.com/Lumisia/aho__ai-handoff/master/scripts/install.ps1))) -Yes -Only codex
#
# This downloads the ai-handoff CLI from GitHub Releases, installs it to
# %USERPROFILE%\.ai-handoff\bin, adds that folder to your user PATH, then runs
# `ai-handoff install`. It mirrors scripts/install.sh for macOS/Linux/WSL.

param(
    [switch]$Yes,
    [switch]$DryRun,
    [ValidateSet('codex', 'claude', 'claude-code')]
    [string]$Only,
    [string]$Version = 'latest',
    [switch]$WithGui
)

$ErrorActionPreference = 'Stop'
$repo = 'Lumisia/aho__ai-handoff'

# TLS 1.2 for Windows PowerShell 5.1 (PowerShell 7+ already defaults to it).
try { [Net.ServicePointManager]::SecurityProtocol = [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12 } catch {}

if ($WithGui) {
    Write-Warning '-WithGui is not available from this CLI installer yet.'
}

# --- Resolve the release artifact for this machine ----------------------------

$archRaw = $env:PROCESSOR_ARCHITECTURE
if ($env:PROCESSOR_ARCHITEW6432) { $archRaw = $env:PROCESSOR_ARCHITEW6432 }
switch ($archRaw) {
    'AMD64' { $arch = 'x86_64' }
    'x86'   { $arch = 'x86_64' }
    'ARM64' { $arch = 'aarch64' }
    default { throw "unsupported architecture: $archRaw" }
}

$artifact = "ai-handoff-cli-windows-$arch.zip"
if ($Version -eq 'latest') {
    $url = "https://github.com/$repo/releases/latest/download/$artifact"
}
else {
    $url = "https://github.com/$repo/releases/download/$Version/$artifact"
}

# --- Paths --------------------------------------------------------------------

$ahHome = if ($env:AI_HANDOFF_HOME) { $env:AI_HANDOFF_HOME } else { Join-Path $env:USERPROFILE '.ai-handoff' }
$binDir = Join-Path $ahHome 'bin'
$tmpDir = Join-Path ([System.IO.Path]::GetTempPath()) ("ai-handoff-install-" + [System.Guid]::NewGuid().ToString('N'))
$archive = Join-Path $tmpDir $artifact
$exeName = 'ai-handoff.exe'
$dest = Join-Path $binDir $exeName

New-Item -ItemType Directory -Force -Path $tmpDir, $binDir | Out-Null

try {
    # --- Download -------------------------------------------------------------
    Write-Host "Downloading $url"
    try {
        Invoke-WebRequest -Uri $url -OutFile $archive -UseBasicParsing
    }
    catch {
        throw @"
Could not download $artifact.
A published GitHub Release with Windows artifacts is required.
URL: $url
Error: $($_.Exception.Message)
"@
    }

    # --- Extract --------------------------------------------------------------
    Expand-Archive -Path $archive -DestinationPath $tmpDir -Force
    $found = Get-ChildItem -Path $tmpDir -Recurse -File -Filter $exeName | Select-Object -First 1
    if (-not $found) {
        throw "artifact did not contain $exeName"
    }

    Copy-Item -Path $found.FullName -Destination $dest -Force
    Write-Host "Installed $dest"

    # --- Add bin dir to the user PATH (and this session) ----------------------
    $userPath = [Environment]::GetEnvironmentVariable('Path', 'User')
    $parts = @()
    if ($userPath) { $parts = $userPath -split ';' }
    if ($parts -notcontains $binDir) {
        $newPath = if ($userPath) { "$userPath;$binDir" } else { $binDir }
        [Environment]::SetEnvironmentVariable('Path', $newPath, 'User')
        Write-Host "Added $binDir to your user PATH (restart terminals to pick it up)."
    }
    if (($env:PATH -split ';') -notcontains $binDir) {
        $env:PATH = "$env:PATH;$binDir"
    }

    # --- Run `ai-handoff install` ---------------------------------------------
    $installArgs = @('install')
    if ($DryRun) { $installArgs += '--dry-run' }
    if ($Yes) { $installArgs += '--yes' }
    if ($Only) { $installArgs += @('--agents', $Only) }

    Write-Host "Running ai-handoff $($installArgs -join ' ')"
    & $dest @installArgs
}
finally {
    Remove-Item -Path $tmpDir -Recurse -Force -ErrorAction SilentlyContinue
}

Write-Host ''
Write-Host 'Done.'
Write-Host "If your shell cannot find ai-handoff, add this directory to PATH:"
Write-Host "  $binDir"

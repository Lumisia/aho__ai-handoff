# AI Handoff — Windows PowerShell installer.
#
# One-line install (downloads and runs this script):
#
#   Set-ExecutionPolicy Bypass -Scope Process -Force; & ([scriptblock]::Create((irm https://raw.githubusercontent.com/Lumisia/aho__ai-handoff/master/scripts/install.ps1))) -Yes
#
# To pass options, fetch the script into a scriptblock instead:
#
#   & ([scriptblock]::Create((irm https://raw.githubusercontent.com/Lumisia/aho__ai-handoff/master/scripts/install.ps1))) -Yes -Only codex
#
# This downloads the ai-handoff CLI from GitHub Releases, installs it to
# %USERPROFILE%\.ai-handoff\bin, adds that folder to your user PATH, then runs
# `ai-handoff install`. The default "latest" version resolves to the highest
# stable vX.Y.Z release tag, not GitHub's mutable "Latest" badge. It mirrors
# scripts/install.sh for macOS/Linux/WSL.

param(
    [switch]$Yes,
    [switch]$DryRun,
    [string]$Only,
    [string]$Version = 'latest',
    [switch]$WithGui
)

$ErrorActionPreference = 'Stop'
$repo = 'Lumisia/aho__ai-handoff'

# Validate -Only here (not via [ValidateSet]) so the empty default does not
# fail validation when this script is run through `irm ... | iex`.
if ($Only -and @('codex', 'claude', 'claude-code') -notcontains $Only) {
    throw "unknown -Only value: $Only (use codex, claude, or claude-code)"
}

# TLS 1.2 for Windows PowerShell 5.1 (PowerShell 7+ already defaults to it).
try { [Net.ServicePointManager]::SecurityProtocol = [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12 } catch {}

function Resolve-LatestReleaseTag {
    param([string]$RepoName)

    $api = "https://api.github.com/repos/$RepoName/releases?per_page=100"
    try {
        $headers = @{
            Accept = 'application/vnd.github+json'
            'User-Agent' = 'ai-handoff-installer'
        }
        $response = Invoke-RestMethod -Uri $api -Headers $headers -UseBasicParsing
        if ($response -is [System.Array]) {
            $releases = $response
        }
        elseif ($null -ne $response) {
            $releases = @($response)
        }
        else {
            $releases = @()
        }
    }
    catch {
        throw @"
Could not resolve the latest ai-handoff release.
URL: $api
Error: $($_.Exception.Message)
"@
    }

    $candidates = @()
    foreach ($release in $releases) {
        if ($release.draft -or $release.prerelease) { continue }

        $tag = [string]$release.tag_name
        if ($tag -match '^v?(\d+)\.(\d+)\.(\d+)$') {
            $published = [datetime]::MinValue
            if ($release.published_at) { $published = [datetime]$release.published_at }
            $candidates += [pscustomobject]@{
                Tag = $tag
                Version = [version]"$($Matches[1]).$($Matches[2]).$($Matches[3])"
                Published = $published
            }
        }
    }

    $latest = $candidates | Sort-Object -Property Version, Published -Descending | Select-Object -First 1
    if (-not $latest) {
        throw "Could not find a stable vX.Y.Z release for $RepoName."
    }

    return $latest.Tag
}

function Download-CheckedFile {
    param(
        [string]$Url,
        [string]$OutFile
    )

    $checksumPath = "$OutFile.sha256"
    Invoke-WebRequest -Uri $Url -OutFile $OutFile -UseBasicParsing
    Invoke-WebRequest -Uri "$Url.sha256" -OutFile $checksumPath -UseBasicParsing
    $expectedHash = ((Get-Content -Path $checksumPath -Raw).Trim() -split '\s+')[0].ToLowerInvariant()
    $actualHash = (Get-FileHash -Path $OutFile -Algorithm SHA256).Hash.ToLowerInvariant()
    if ($actualHash -ne $expectedHash) {
        throw "Checksum mismatch for $(Split-Path -Leaf $OutFile). Expected $expectedHash, got $actualHash."
    }
}

function Install-Gui {
    param(
        [string]$Url,
        [string]$Artifact,
        [string]$TmpDir,
        [bool]$Silent
    )

    $installer = Join-Path $TmpDir $Artifact
    Write-Host "Downloading $Url"
    try {
        Download-CheckedFile -Url $Url -OutFile $installer
    }
    catch {
        throw @"
Could not download $Artifact.
A published GitHub Release with Windows GUI artifacts is required.
URL: $Url
Error: $($_.Exception.Message)
"@
    }

    $installerArgs = @()
    if ($Silent) { $installerArgs += '/S' }
    Write-Host "Running GUI installer $Artifact"
    $process = Start-Process -FilePath $installer -ArgumentList $installerArgs -Wait -PassThru
    if ($process.ExitCode -ne 0) {
        throw "GUI installer failed with exit code $($process.ExitCode)."
    }
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
$guiArtifact = "ai-handoff-gui-windows-$arch-setup.exe"
$releaseVersion = $Version
if ($Version -eq 'latest') {
    $releaseVersion = Resolve-LatestReleaseTag -RepoName $repo
    Write-Host "Resolved latest release: $releaseVersion"
}
$url = "https://github.com/$repo/releases/download/$releaseVersion/$artifact"
$guiUrl = "https://github.com/$repo/releases/download/$releaseVersion/$guiArtifact"

# --- Paths --------------------------------------------------------------------

$ahHome = if ($env:AI_HANDOFF_HOME) { $env:AI_HANDOFF_HOME } else { Join-Path $env:USERPROFILE '.ai-handoff' }
$binDir = Join-Path $ahHome 'bin'
$tmpDir = Join-Path ([System.IO.Path]::GetTempPath()) ("ai-handoff-install-" + [System.Guid]::NewGuid().ToString('N'))
$archive = Join-Path $tmpDir $artifact
$exeName = 'ai-handoff.exe'
$hostExeName = 'ai-handoff-host.exe'
$dest = Join-Path $binDir $exeName
$hostDest = Join-Path $binDir $hostExeName

New-Item -ItemType Directory -Force -Path $tmpDir, $binDir | Out-Null

try {
    # --- Download -------------------------------------------------------------
    Write-Host "Downloading $url"
    try {
        Download-CheckedFile -Url $url -OutFile $archive
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
    $hostFound = Get-ChildItem -Path $tmpDir -Recurse -File -Filter $hostExeName | Select-Object -First 1
    if (-not $hostFound) {
        throw "artifact did not contain $hostExeName"
    }

    # Clean up any leftover from a previous in-place update (best effort: the
    # old exe may still be running).
    Remove-Item -Path "$dest.old", "$hostDest.old" -Force -ErrorAction SilentlyContinue

    foreach ($binary in @(
        @{ Source = $found.FullName; Destination = $dest },
        @{ Source = $hostFound.FullName; Destination = $hostDest }
    )) {
        try {
            Copy-Item -Path $binary.Source -Destination $binary.Destination -Force
        }
        catch {
            # A running executable can be renamed even when it cannot be
            # overwritten. Move it aside, then install the fresh binary.
            Move-Item -Path $binary.Destination -Destination "$($binary.Destination).old" -Force
            Copy-Item -Path $binary.Source -Destination $binary.Destination -Force
        }
    }
    Write-Host "Installed $dest"
    Write-Host "Installed $hostDest"

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

    if ($WithGui) {
        Install-Gui -Url $guiUrl -Artifact $guiArtifact -TmpDir $tmpDir -Silent ([bool]$Yes)
    }
}
finally {
    Remove-Item -Path $tmpDir -Recurse -Force -ErrorAction SilentlyContinue
}

Write-Host ''
Write-Host 'Done.'
Write-Host "If your shell cannot find ai-handoff, add this directory to PATH:"
Write-Host "  $binDir"

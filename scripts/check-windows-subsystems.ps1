param(
    [Parameter(Mandatory = $true)]
    [string]$CliPath,
    [Parameter(Mandatory = $true)]
    [string]$HostPath
)

$ErrorActionPreference = 'Stop'

function Get-PeSubsystem {
    param([string]$Path)

    $resolved = (Resolve-Path -LiteralPath $Path).Path
    $bytes = [System.IO.File]::ReadAllBytes($resolved)
    if ($bytes.Length -lt 0x40 -or $bytes[0] -ne 0x4d -or $bytes[1] -ne 0x5a) {
        throw "$resolved is not a valid PE executable"
    }

    $peOffset = [System.BitConverter]::ToInt32($bytes, 0x3c)
    $signature = [System.Text.Encoding]::ASCII.GetString($bytes, $peOffset, 4)
    if ($signature -ne "PE`0`0") {
        throw "$resolved has an invalid PE signature"
    }

    # IMAGE_OPTIONAL_HEADER.Subsystem is 68 bytes into both PE32 and PE32+.
    return [System.BitConverter]::ToUInt16($bytes, $peOffset + 24 + 68)
}

$cliSubsystem = Get-PeSubsystem -Path $CliPath
$hostSubsystem = Get-PeSubsystem -Path $HostPath

if ($cliSubsystem -ne 3) {
    throw "CLI subsystem must be Windows CUI (3), got $cliSubsystem"
}
if ($hostSubsystem -ne 2) {
    throw "native host subsystem must be Windows GUI (2), got $hostSubsystem"
}

Write-Host "Windows PE subsystems valid: cli=CUI(3), host=GUI(2)"

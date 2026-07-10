param(
    [Parameter(Mandatory = $true)]
    [string]$InstallerPath,

    [Parameter(Mandatory = $true)]
    [string]$ExpectedIconPath
)

$ErrorActionPreference = 'Stop'

$resolvedInstaller = (Resolve-Path -LiteralPath $InstallerPath).Path
$resolvedExpected = (Resolve-Path -LiteralPath $ExpectedIconPath).Path

Add-Type -AssemblyName System.Drawing

$actualIcon = [System.Drawing.Icon]::ExtractAssociatedIcon($resolvedInstaller)
if ($null -eq $actualIcon) {
    throw "installer has no associated icon: $resolvedInstaller"
}

$expectedIcon = New-Object System.Drawing.Icon($resolvedExpected, 32, 32)

try {
    $actualBitmap = $actualIcon.ToBitmap()
    $expectedBitmap = $expectedIcon.ToBitmap()

    try {
        if ($actualBitmap.Width -ne 32 -or $actualBitmap.Height -ne 32) {
            throw "actual icon is $($actualBitmap.Width)x$($actualBitmap.Height), expected 32x32"
        }

        for ($y = 0; $y -lt 32; $y++) {
            for ($x = 0; $x -lt 32; $x++) {
                $actualArgb = $actualBitmap.GetPixel($x, $y).ToArgb()
                $expectedArgb = $expectedBitmap.GetPixel($x, $y).ToArgb()
                if ($actualArgb -ne $expectedArgb) {
                    throw "installer icon mismatch at ($x,$y)"
                }
            }
        }
    }
    finally {
        $actualBitmap.Dispose()
        $expectedBitmap.Dispose()
    }
}
finally {
    $actualIcon.Dispose()
    $expectedIcon.Dispose()
}

Write-Output 'installer icon verified'

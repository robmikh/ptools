[CmdletBinding()]
param(
    [Parameter(Mandatory)]
    [ValidateSet("amd64", "arm64")]
    [string]$Architecture,

    [Parameter(Mandatory)]
    [ValidateSet("x86_64-pc-windows-msvc", "aarch64-pc-windows-msvc")]
    [string]$Target,

    [Parameter(Mandatory)]
    [ValidatePattern("^v[0-9A-Za-z][0-9A-Za-z._-]*$")]
    [string]$Version,

    [string]$OutputDirectory = "dist"
)

$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

$repositoryRoot = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
$outputRoot = if ([System.IO.Path]::IsPathRooted($OutputDirectory)) {
    $OutputDirectory
} else {
    Join-Path $repositoryRoot $OutputDirectory
}
$packageName = "ptools-$Version-windows-$Architecture"
$stagingDirectory = Join-Path $outputRoot $packageName
$archivePath = Join-Path $outputRoot "$packageName.zip"

Push-Location $repositoryRoot
try {
    $metadataJson = & cargo metadata --locked --no-deps --format-version 1
    if ($LASTEXITCODE -ne 0) {
        throw "cargo metadata failed with exit code $LASTEXITCODE."
    }
    $metadata = $metadataJson | ConvertFrom-Json

    $binaryNames = @(
        foreach ($package in $metadata.packages) {
            if ($metadata.workspace_members -notcontains $package.id) {
                continue
            }
            foreach ($targetInfo in $package.targets) {
                if ($targetInfo.kind -contains "bin") {
                    $targetInfo.name
                }
            }
        }
    )

    if ($binaryNames.Count -eq 0) {
        throw "No binary targets were found in the Cargo workspace."
    }

    $duplicate = $binaryNames |
        Group-Object |
        Where-Object { $_.Count -gt 1 } |
        Select-Object -First 1
    if ($null -ne $duplicate) {
        throw "More than one workspace binary is named '$($duplicate.Name)'."
    }

    New-Item -ItemType Directory -Path $outputRoot -Force | Out-Null
    if (Test-Path -LiteralPath $stagingDirectory) {
        Remove-Item -LiteralPath $stagingDirectory -Recurse -Force
    }
    New-Item -ItemType Directory -Path $stagingDirectory | Out-Null

    foreach ($binaryName in $binaryNames) {
        $source = Join-Path $repositoryRoot "target\$Target\release\$binaryName.exe"
        if (-not (Test-Path -LiteralPath $source -PathType Leaf)) {
            throw "Expected workspace binary '$binaryName' at '$source'."
        }
        Copy-Item -LiteralPath $source -Destination $stagingDirectory
    }

    if (Test-Path -LiteralPath $archivePath) {
        Remove-Item -LiteralPath $archivePath -Force
    }
    Compress-Archive -Path (Join-Path $stagingDirectory "*") -DestinationPath $archivePath
    Remove-Item -LiteralPath $stagingDirectory -Recurse -Force

    Write-Output $archivePath
} finally {
    Pop-Location
}

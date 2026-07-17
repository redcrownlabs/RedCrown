param(
    [switch]$Quiet
)

$ErrorActionPreference = 'Stop'

$root = Split-Path -Parent $PSScriptRoot
$toolsRoot = Join-Path $root '.redcrown\tools'
$toolId = 'ffmpeg-n8.1.2-22-g94138f6973-win64-lgpl-shared-8.1'
$toolRoot = Join-Path $toolsRoot $toolId
$binRoot = Join-Path $toolRoot 'bin'
$ffmpeg = Join-Path $binRoot 'ffmpeg.exe'
$ffprobe = Join-Path $binRoot 'ffprobe.exe'
$archiveName = "$toolId.zip"
$archive = Join-Path $toolsRoot $archiveName
$archiveUrl = "https://github.com/BtbN/FFmpeg-Builds/releases/download/autobuild-2026-07-17-13-22/$archiveName"
$expectedSha256 = 'fcbf0f5c58fec3e516e35ba26d81bc6cbaea09dde76bffd151fa93c0316b0b50'

if (-not ((Test-Path -LiteralPath $ffmpeg -PathType Leaf) -and (Test-Path -LiteralPath $ffprobe -PathType Leaf))) {
    New-Item -ItemType Directory -Path $toolsRoot -Force | Out-Null
    if (-not (Test-Path -LiteralPath $archive -PathType Leaf)) {
        if (-not $Quiet) { Write-Host "Downloading pinned FFmpeg $toolId..." }
        Invoke-WebRequest -Uri $archiveUrl -OutFile $archive
    }

    $actualSha256 = (Get-FileHash -LiteralPath $archive -Algorithm SHA256).Hash.ToLowerInvariant()
    if ($actualSha256 -ne $expectedSha256) {
        throw "FFmpeg archive checksum mismatch. Expected $expectedSha256 but received $actualSha256."
    }

    if (Test-Path -LiteralPath $toolRoot) {
        throw "Incomplete FFmpeg tool directory already exists at $toolRoot. Remove that directory before retrying."
    }
    if (-not $Quiet) { Write-Host 'Extracting verified FFmpeg toolchain...' }
    $stagingRoot = Join-Path $toolsRoot ".ffmpeg-extract-$([guid]::NewGuid().ToString('N'))"
    Expand-Archive -LiteralPath $archive -DestinationPath $stagingRoot
    $stagedToolRoot = Join-Path $stagingRoot $toolId
    $stagedFfmpeg = Join-Path $stagedToolRoot 'bin\ffmpeg.exe'
    $stagedFfprobe = Join-Path $stagedToolRoot 'bin\ffprobe.exe'
    if (-not ((Test-Path -LiteralPath $stagedFfmpeg -PathType Leaf) -and (Test-Path -LiteralPath $stagedFfprobe -PathType Leaf))) {
        throw "Verified FFmpeg archive did not contain the expected executables. Staging was retained at $stagingRoot for inspection."
    }
    Move-Item -LiteralPath $stagedToolRoot -Destination $toolRoot
    if (-not ((Test-Path -LiteralPath $ffmpeg -PathType Leaf) -and (Test-Path -LiteralPath $ffprobe -PathType Leaf))) {
        throw "Verified FFmpeg archive did not contain the expected executables under $binRoot."
    }
}

[pscustomobject]@{
    Ffmpeg = $ffmpeg
    Ffprobe = $ffprobe
    ToolRoot = $toolRoot
}

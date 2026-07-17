param(
    [switch]$SkipChecks
)

$ErrorActionPreference = 'Stop'

if ($env:OS -ne 'Windows_NT') {
    throw 'Windows packaging must run on Windows.'
}

$root = Split-Path -Parent $PSScriptRoot
$desktop = Join-Path $root 'apps\desktop'
$backendManifest = Join-Path $root 'backend\Cargo.toml'
$releaseResources = Join-Path $root '.redcrown\release-resources'
$releaseOutput = Join-Path $desktop 'release'

function Assert-CommandSucceeded {
    param(
        [Parameter(Mandatory = $true)]
        [string]$Name,
        [Parameter(Mandatory = $true)]
        [int]$ExitCode
    )

    if ($ExitCode -ne 0) {
        throw "$Name failed with exit code $ExitCode."
    }
}

if (-not $SkipChecks) {
    & (Join-Path $PSScriptRoot 'tasks.ps1') fmt
    & (Join-Path $PSScriptRoot 'tasks.ps1') lint
    & (Join-Path $PSScriptRoot 'tasks.ps1') test
}

$mediaTools = & (Join-Path $PSScriptRoot 'ensure-ffmpeg.ps1') -Quiet

cargo build --locked --release --manifest-path $backendManifest -p redcrown-desktop
Assert-CommandSucceeded 'Release backend build' $LASTEXITCODE

npm run build --prefix $desktop
Assert-CommandSucceeded 'Desktop production build' $LASTEXITCODE

$resolvedResources = [System.IO.Path]::GetFullPath($releaseResources)
$allowedResources = [System.IO.Path]::GetFullPath((Join-Path $root '.redcrown')) + [System.IO.Path]::DirectorySeparatorChar
if (-not $resolvedResources.StartsWith($allowedResources, [System.StringComparison]::OrdinalIgnoreCase)) {
    throw 'Release resource staging escaped the private build directory.'
}
if (Test-Path -LiteralPath $resolvedResources) {
    Remove-Item -LiteralPath $resolvedResources -Recurse -Force
}

$binTarget = Join-Path $resolvedResources 'bin'
$ffmpegTarget = Join-Path $resolvedResources 'ffmpeg'
$licenseTarget = Join-Path $resolvedResources 'licenses'
New-Item -ItemType Directory -Force -Path $binTarget, $ffmpegTarget, $licenseTarget | Out-Null

$backendBinary = Join-Path $root 'backend\target\release\redcrown-desktop.exe'
Copy-Item -LiteralPath $backendBinary -Destination (Join-Path $binTarget 'redcrown-backend.exe')
Copy-Item -LiteralPath $mediaTools.Ffmpeg -Destination $ffmpegTarget
Copy-Item -LiteralPath $mediaTools.Ffprobe -Destination $ffmpegTarget
Get-ChildItem -LiteralPath (Split-Path $mediaTools.Ffmpeg) -Filter '*.dll' -File |
    Copy-Item -Destination $ffmpegTarget
Copy-Item -LiteralPath (Join-Path $mediaTools.ToolRoot 'LICENSE.txt') -Destination (Join-Path $licenseTarget 'FFmpeg-LICENSE.txt')
Copy-Item -LiteralPath (Join-Path $root 'LICENSE') -Destination (Join-Path $licenseTarget 'RedCrown-LICENSE.txt')

$requiredResources = @(
    (Join-Path $binTarget 'redcrown-backend.exe'),
    (Join-Path $ffmpegTarget 'ffmpeg.exe'),
    (Join-Path $ffmpegTarget 'ffprobe.exe'),
    (Join-Path $licenseTarget 'FFmpeg-LICENSE.txt'),
    (Join-Path $licenseTarget 'RedCrown-LICENSE.txt')
)
foreach ($required in $requiredResources) {
    if (-not (Test-Path -LiteralPath $required -PathType Leaf)) {
        throw "Required release resource is missing: $required"
    }
}
if ((Get-ChildItem -LiteralPath $ffmpegTarget -Filter '*.dll' -File | Measure-Object).Count -eq 0) {
    throw 'The staged shared FFmpeg runtime does not contain its required DLLs.'
}

$previousCodeSignDiscovery = $env:CSC_IDENTITY_AUTO_DISCOVERY
$previousPath = $env:PATH
$env:CSC_IDENTITY_AUTO_DISCOVERY = 'false'
# electron-builder invokes npm.cmd through legacy powershell.exe to avoid the
# unsafe .cmd spawning behavior fixed by Node. Minimal Windows environments can
# omit its standard directory from PATH even though the OS component exists.
$legacyPowerShellDirectory = Join-Path $env:SystemRoot 'System32\WindowsPowerShell\v1.0'
$legacyPowerShell = Join-Path $legacyPowerShellDirectory 'powershell.exe'
if (-not (Get-Command 'powershell.exe' -ErrorAction SilentlyContinue)) {
    if (-not (Test-Path -LiteralPath $legacyPowerShell -PathType Leaf)) {
        throw "electron-builder requires the Windows PowerShell component at $legacyPowerShell."
    }
    $env:PATH = "$legacyPowerShellDirectory;$env:PATH"
}
try {
    npm run package:win --prefix $desktop
    Assert-CommandSucceeded 'Windows desktop packaging' $LASTEXITCODE
} finally {
    $env:CSC_IDENTITY_AUTO_DISCOVERY = $previousCodeSignDiscovery
    $env:PATH = $previousPath
}

$artifacts = Get-ChildItem -LiteralPath $releaseOutput -File |
    Where-Object { $_.Extension -in '.exe', '.zip' } |
    Sort-Object Name
if (($artifacts | Measure-Object).Count -ne 2) {
    throw 'Packaging must produce exactly one installer and one ZIP artifact.'
}

$packagedResources = Join-Path $releaseOutput 'win-unpacked\resources'
foreach ($relative in @(
    'bin\redcrown-backend.exe',
    'ffmpeg\ffmpeg.exe',
    'ffmpeg\ffprobe.exe',
    'licenses\FFmpeg-LICENSE.txt',
    'licenses\RedCrown-LICENSE.txt'
)) {
    $packaged = Join-Path $packagedResources $relative
    if (-not (Test-Path -LiteralPath $packaged -PathType Leaf)) {
        throw "Packaged application is missing required resource: $relative"
    }
}
if ((Get-ChildItem -LiteralPath (Join-Path $packagedResources 'ffmpeg') -Filter '*.dll' -File | Measure-Object).Count -eq 0) {
    throw 'Packaged application is missing the shared FFmpeg runtime DLLs.'
}

$checksumLines = foreach ($artifact in $artifacts) {
    $hash = (Get-FileHash -LiteralPath $artifact.FullName -Algorithm SHA256).Hash.ToLowerInvariant()
    "$hash  $($artifact.Name)"
}
$checksumPath = Join-Path $releaseOutput 'SHA256SUMS.txt'
[System.IO.File]::WriteAllLines($checksumPath, $checksumLines, [System.Text.UTF8Encoding]::new($false))

$artifacts | Select-Object Name, Length, FullName
Get-Item -LiteralPath $checksumPath | Select-Object Name, Length, FullName

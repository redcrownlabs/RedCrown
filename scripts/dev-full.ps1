$ErrorActionPreference = 'Stop'

$root = Split-Path -Parent $PSScriptRoot
$desktop = Join-Path $root 'apps\desktop'
$backend = Join-Path $root 'backend'

if (-not (Test-Path (Join-Path $desktop 'node_modules'))) {
    npm install --prefix $desktop
}

$mediaTools = & (Join-Path $PSScriptRoot 'ensure-ffmpeg.ps1')
$env:REDCROWN_FFMPEG_BIN = $mediaTools.Ffmpeg
$env:REDCROWN_FFPROBE_BIN = $mediaTools.Ffprobe

cargo build --manifest-path (Join-Path $backend 'Cargo.toml') -p redcrown-desktop
if ($LASTEXITCODE -ne 0) {
    exit $LASTEXITCODE
}
$env:REDCROWN_BACKEND_BIN = Join-Path $backend 'target\debug\redcrown-desktop.exe'
npm run dev --prefix $desktop

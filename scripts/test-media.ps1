$ErrorActionPreference = 'Stop'

$root = Split-Path -Parent $PSScriptRoot
$mediaTools = & (Join-Path $PSScriptRoot 'ensure-ffmpeg.ps1') -Quiet
$env:REDCROWN_FFMPEG_BIN = $mediaTools.Ffmpeg
$env:REDCROWN_FFPROBE_BIN = $mediaTools.Ffprobe

cargo test --manifest-path (Join-Path $root 'backend\Cargo.toml') -p redcrown-torrent integration_tests::media_bridge_transcodes_audio_and_exposes_subtitles -- --ignored --exact
if ($LASTEXITCODE -ne 0) {
    exit $LASTEXITCODE
}

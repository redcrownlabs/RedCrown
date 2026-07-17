$ErrorActionPreference = 'Stop'

$root = Split-Path -Parent $PSScriptRoot
$manifest = Join-Path $root 'backend\Cargo.toml'

cargo test `
    --manifest-path $manifest `
    -p redcrown-torrent `
    integration_tests::downloads_prebuffers_and_serves_from_a_local_seed `
    -- `
    --nocapture

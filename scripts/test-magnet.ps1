param(
    [Parameter(Mandatory = $true)]
    [ValidatePattern('^magnet:\?')]
    [string]$Magnet,
    [ValidateRange(5, 600)]
    [int]$MetadataTimeoutSeconds = 10,
    [ValidateRange(5, 600)]
    [int]$TransferTimeoutSeconds = 180
)

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
$manifest = Join-Path $root 'backend\Cargo.toml'
$env:REDCROWN_TEST_MAGNET = $Magnet
$env:REDCROWN_TEST_METADATA_TIMEOUT_SECONDS = $MetadataTimeoutSeconds
$env:REDCROWN_TEST_TRANSFER_TIMEOUT_SECONDS = $TransferTimeoutSeconds

try {
    cargo test `
        --manifest-path $manifest `
        -p redcrown-torrent `
        integration_tests::resolves_and_downloads_from_external_magnet `
        -- `
        --ignored `
        --exact `
        --nocapture
    if ($LASTEXITCODE -ne 0) {
        throw "External magnet transfer test failed with exit code $LASTEXITCODE."
    }
}
finally {
    Remove-Item Env:REDCROWN_TEST_MAGNET -ErrorAction SilentlyContinue
    Remove-Item Env:REDCROWN_TEST_METADATA_TIMEOUT_SECONDS -ErrorAction SilentlyContinue
    Remove-Item Env:REDCROWN_TEST_TRANSFER_TIMEOUT_SECONDS -ErrorAction SilentlyContinue
}

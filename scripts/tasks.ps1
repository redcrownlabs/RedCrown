param(
    [Parameter(Mandatory = $true)]
    [ValidateSet('test', 'lint', 'build', 'fmt')]
    [string]$Task
)

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
$desktop = Join-Path $root 'apps\desktop'
$backendManifest = Join-Path $root 'backend\Cargo.toml'

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

switch ($Task) {
    'test' {
        cargo test --manifest-path $backendManifest --workspace --exclude librqbit
        Assert-CommandSucceeded 'Rust tests' $LASTEXITCODE
        npm test --prefix $desktop
        Assert-CommandSucceeded 'Desktop tests' $LASTEXITCODE
    }
    'lint' {
        cargo clippy --manifest-path $backendManifest --workspace --all-targets --no-deps `
            --exclude librqbit --exclude librqbit-tracker-comms --exclude librqbit-upnp `
            --exclude librqbit-upnp-serve `
            -- -D warnings
        Assert-CommandSucceeded 'Rust lint' $LASTEXITCODE
        npm run lint --prefix $desktop
        Assert-CommandSucceeded 'Desktop lint' $LASTEXITCODE
    }
    'build' {
        cargo build --manifest-path $backendManifest --workspace
        Assert-CommandSucceeded 'Rust build' $LASTEXITCODE
        npm run build --prefix $desktop
        Assert-CommandSucceeded 'Desktop build' $LASTEXITCODE
    }
    'fmt' {
        cargo fmt --manifest-path $backendManifest --all -- --check
        Assert-CommandSucceeded 'Rust formatting check' $LASTEXITCODE
    }
}

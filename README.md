# RedCrown

RedCrown is a Windows-first, watch-now desktop media player with a Rust backend
and a sandboxed Electron/React interface. It is designed around temporary,
expiring stream cache rather than permanent downloads.

> [!IMPORTANT]
> RedCrown is under active development. Published builds are unsigned
> pre-releases; signed updates and broader real-network qualification are still
> in progress.

## What is implemented

- movies, series, episodes, search, sorting, and catalog pagination;
- ordered, configurable Popcorn Time-compatible API fallback URLs;
- source and quality selection with torrent size and filename details;
- Rust-owned BitTorrent metadata resolution and prioritized streaming through
  `librqbit`;
- bounded supplemental tracker import for trackerless magnets from a
  configurable HTTPS URL or absolute local file;
- loopback-only, tokenized HTTP range playback;
- FFmpeg compatibility bridging with audio-track and subtitle selection;
- favorites, exact movie/episode watched state, Continue Watching, and Popcorn
  Time settings/library import;
- lease-aware, expiring stream cache with size-pressure eviction;
- torrent diagnostics and optional, explicitly redacted OpenTelemetry export.

Design decisions, implemented behavior, and security invariants live in
[docs/](docs/). Remaining release limitations are stated explicitly in this
README rather than presented as finished features.

## Architecture

```text
Electron main process
  ├─ sandboxed React renderer (presentation only)
  └─ supervised Rust backend over inherited JSON-lines stdio
       ├─ catalog endpoint fallback and normalization
       ├─ temporary library and settings persistence
       ├─ librqbit torrent session and cache ownership
       ├─ loopback playback server and FFmpeg bridge
       └─ local diagnostics and optional OTLP traces
```

Sensitive networking, filesystem access, torrent state, cache deletion, and
telemetry redaction remain on the Rust side of the process boundary. See
[docs/architecture.md](docs/architecture.md) and
[docs/security-model.md](docs/security-model.md) for the reasoning and
invariants.

## Prerequisites

- Windows 10 or newer;
- PowerShell 7;
- Rust 1.88 or newer with Cargo, rustfmt, and Clippy;
- Node.js 24 and npm;
- network access for Rust/npm dependencies and the pinned FFmpeg archive.

The development launcher downloads an LGPL FFmpeg build from its upstream
release, verifies its SHA-256 digest, and stores it under the ignored
`.redcrown/` directory. It does not execute an unverified media binary or
search the machine-wide `PATH` for FFmpeg.

## Development

Clone the repository and start the complete desktop stack:

```powershell
git clone https://github.com/redcrownlabs/RedCrown.git
Set-Location RedCrown
./scripts/dev-full.ps1
```

On a clean first launch, RedCrown opens the source settings screen because the
repository does not embed a third-party catalog endpoint. Add one or more
compatible API base URLs in fallback order, select **Test all**, and then
**Save**. Once at least one source is configured and returns catalog rows, the
app opens Home. Configuration is stored in the operating system's per-user
application-data directory, never in the repository checkout.

Tracker import is enabled initially and points to the daily public
`trackers_all.txt` list maintained by
[ngosang/trackerslist](https://github.com/ngosang/trackerslist). Settings can
disable it or select another HTTPS URL or absolute local file. RedCrown caps
imports at 1 MiB and 512 unique trackers and supplements only magnets that have
no tracker of their own.

Watched movies are hidden from Home and movie discovery by default. Change
**Settings → Discovery → Hide watched movies** to keep them visible. Right-click
a media card to mark a movie or currently known series as watched. Series detail
pages retain per-episode controls, while Continue Watching offers the first
playable regular episode not explicitly marked watched. Removing a series from
Continue Watching does not erase its watched history and can be reversed from
the same card context menu.

## Pre-release builds

Windows installer and ZIP builds are published on the
[GitHub Releases](https://github.com/redcrownlabs/RedCrown/releases) page with a
`SHA256SUMS.txt` integrity manifest. They contain the Rust backend and the pinned
shared FFmpeg runtime, so FFmpeg does not need to be installed separately.

The current builds are not code-signed. Windows may show an unknown-publisher
warning; verify the downloaded artifact against the attached SHA-256 manifest.
The release design and promotion process are documented in
[docs/release-process.md](docs/release-process.md).

Run the same checks used by continuous integration:

```powershell
npm ci --prefix ./apps/desktop
./scripts/tasks.ps1 fmt
./scripts/tasks.ps1 lint
./scripts/tasks.ps1 test
./scripts/tasks.ps1 build
```

The focused media integration test creates its own local torrent and synthetic
media fixture; it does not depend on a public swarm:

```powershell
./scripts/test-media.ps1
```

Qualify a specific public magnet outside Electron with:

```powershell
./scripts/test-magnet.ps1 -Magnet '<magnet URI>'
```

## OpenTelemetry

Trace export is disabled by default. To export the deliberately restricted
telemetry surface to an OTLP/HTTP collector:

```powershell
$env:REDCROWN_OTEL_ENABLED = '1'
$env:OTEL_EXPORTER_OTLP_ENDPOINT = 'http://127.0.0.1:4318'
./scripts/dev-full.ps1
```

Normal detailed logs are kept local and are not forwarded by the OTLP layer.
Standard OTLP endpoint and header environment variables are supported; treat
header values as secrets and never commit them.

## Responsible use

RedCrown does not ship media, catalog services, tracker-list contents, or API
credentials. Its initial settings reference a third-party public tracker-list
URL, which users can disable or replace. Users and downstream distributors are responsible for configuring
lawful sources and complying with copyright law and the terms of services they
use. The project is not affiliated with Popcorn Time, Netflix, TMDB, or any
content provider.

## Contributing and security

Read [CONTRIBUTING.md](CONTRIBUTING.md) before submitting a change. Please
report vulnerabilities privately as described in [SECURITY.md](SECURITY.md),
not through a public issue.

## License

RedCrown is available under the [MIT License](LICENSE). Vendored `librqbit`
components retain their upstream Apache-2.0 license; their provenance, security
maintenance, and local patch rationale are documented in each
`backend/third_party/*/VENDORED.md`.

# Security Model

Date: 2026-07-16
Status: Initial accepted baseline

## Protected assets

- local files and user paths;
- source configuration and secret headers;
- torrent cache contents;
- backend process authority;
- future update signing keys and manifests.

## Controls

- Electron renderer sandbox and context isolation are enabled.
- Node integration is disabled.
- Preload exposes named commands only.
- Child windows and external navigation are denied in the current slice.
- Backend IPC uses inherited pipes and bounded JSON lines.
- API URLs accept only HTTP(S) and reject embedded credentials.
- Playback HTTP endpoints bind to loopback and use per-process session tokens.
- Stream-cache deletion accepts only manifest-backed, lowercase info-hash
  directories that canonicalize to direct children of the configured root.
- OTLP export is disabled by default and receives only events or spans emitted
  to the dedicated redacted telemetry target.

## Known initial limitation

The first slice does not yet implement signed application updates, OS
credential storage, or a user-facing external link allowlist. No secret source
headers are accepted until protected storage is implemented.

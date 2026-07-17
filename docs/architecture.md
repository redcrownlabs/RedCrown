# Architecture

Date: 2026-07-16
Status: Accepted

## Requirement

RedCrown must provide reliable watch-now torrent streaming while retaining a
small, modern desktop UI. The renderer must not own torrenting, networking,
filesystem access, source credentials, cache cleanup, or update verification.

## Decision

The repository is divided by authority:

- `apps/desktop` owns Electron lifecycle, preload isolation, and React
  presentation.
- `backend/crates/redcrown-core` owns stable domain and IPC types.
- `backend/crates/redcrown-catalog` owns endpoint fallback and API
  normalization.
- `backend/crates/redcrown-torrent` owns the `librqbit` boundary.
- `backend/crates/redcrown-desktop` owns process assembly, persisted settings,
  and the local command protocol.
- `backend/crates/redcrown-diagnostics` owns local tracing composition,
  telemetry isolation, and optional OpenTelemetry/OTLP trace export.

Electron starts one supervised Rust child. Communication uses inherited stdio
with newline-delimited, versioned JSON messages. Inherited pipes are preferred
over a listening local port for the first Windows implementation because they
do not create a discoverable IPC endpoint and naturally share the child
lifetime. This can later move to Windows named pipes without changing renderer
contracts.

## Invariants

- The renderer receives only typed projections.
- Third-party torrent types never cross the `redcrown-torrent` crate boundary.
- Source endpoint changes are validated and explicitly saved.
- Torrent bytes are temporary cache, not user downloads.
- Active cache leases cannot be evicted.
- A failed backend request returns a typed error and never hangs indefinitely.
- Telemetry export is optional, bounded, redacted, and cannot become a
  dependency of playback correctness.

## Observability boundary

Domain crates instrument work with stable `tracing` span and event names. The
desktop host composes the local structured-log layer and, only when explicitly
configured, an OpenTelemetry layer that exports OTLP to a user-selected
collector. Export failures are isolated from application work and fall back to
local diagnostics.

The exporter subscribes only to the dedicated `redcrown_telemetry` target.
Normal module events—including detailed error messages—are excluded even when
OTLP is enabled. Safe IPC spans map arbitrary renderer method input to a fixed
operation allowlist before export. This target separation is the redaction
boundary; adding an exported field requires an explicit privacy review.

Telemetry attributes may contain technical identifiers, durations, byte
counts, state transitions, and bounded error categories. They must not contain
media titles, magnet or tracker URLs, source credentials, subtitle content,
browsing history, or full user paths.

## Settings durability

Settings use a two-slot generation journal. Each save is synchronized to a
unique temporary file before replacing the older slot; startup validates both
slots and selects the highest valid generation. A crash during save must always
leave the previous known-good configuration recoverable.

## First-run source configuration

The public repository and application package deliberately contain no catalog
service URL. Catalog providers are independently operated, can disappear or
change policy, and may be lawful in one deployment but inappropriate in
another. A clean settings store therefore starts with an enabled logical source
and no endpoints.

The renderer treats that state as configuration-required, skips catalog network
requests, and opens the source settings screen. Home becomes the active view
only after the user saves at least one enabled endpoint and the fallback chain
returns catalog rows. This preserves the invariant that a public build has no
maintainer-specific service dependency while still giving a clean installation
a deterministic path to a usable state. The accepted tradeoff is one explicit
setup step instead of an opaque or unstable bundled default.

## Verification feature graph

Continuous integration lints RedCrown-owned crates and tests and builds the
complete locked default Cargo feature graph used by the desktop product. The
vendored `librqbit` workspace members are excluded from RedCrown's style lint so
unrelated formatting churn does not undermine the minimal-fork invariant; they
remain compiled and tested as workspace members. Their optional upstream
server, database, and TLS combinations are not enabled because `--all-features`
would validate a different product and require additional native toolchains
such as NASM. Focused changes to a vendored optional feature must qualify that
feature separately before enabling it in the RedCrown dependency graph.

## Stream-cache ownership

Every managed torrent writes beneath a direct child directory named by its
lowercase 40-character info hash. `redcrown-torrent` passes that directory to
`librqbit` through its public `sub_folder` option; no engine-private paths are
inspected.

Each cache entry has a two-slot generation manifest containing creation and
last-access timestamps. Active playback holds an in-memory lease. Cleanup is
serialized with lease changes, skips leased entries, removes expired entries,
then applies least-recently-used eviction until the size budget is met.
Unknown directories, symlinks, invalid identifiers, and paths that do not
canonicalize to a direct child of the configured stream-cache root are never
deleted.

DHT routing state is stored as a RedCrown-owned file at the cache root, outside
info-hash media directories. It survives media expiration so a later launch can
reuse learned peers when public bootstrap DNS is unavailable. Torrent session
membership is deliberately not persisted: only discovery state crosses an
application restart.

## Tradeoff

The Electron shell costs more memory than a native Rust UI. It is accepted
because the requested Loom-style process model is already understood, React
supports the intended product experience, and all sensitive/high-throughput
work remains in Rust.

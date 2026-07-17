# Popcorn Time Migration Design

Date: 2026-07-16

## Requirement

RedCrown must let a user carry forward compatible Popcorn Time configuration,
favorites, watched movies, watched episodes, and playback progress without
copying Popcorn Time's interface or mutating its profile.

## Invariants

- Discovery is limited to known Windows profile locations. The renderer never
  supplies or receives an arbitrary filesystem path.
- Source files are opened read-only, bounded by file and record size, and
  interpreted using NeDB's append/update/tombstone behavior.
- Imports are previewed and explicitly selected by category.
- API URLs must be HTTP(S), contain no credentials, and are deduplicated before
  being appended to RedCrown's existing ordered fallback chain.
- Tokens, passwords, local IP addresses, player paths, DHT keys, and unrelated
  Popcorn settings are never imported.
- Durable library records use stable provider keys and idempotent SQLite
  upserts. Repeating an import cannot create duplicates or clear newer flags.
- Watched movie and episode records are committed in one SQLite transaction.
- Playback progress is imported only when a positive position can be matched to
  exactly one known media identity. A title-only guess is rejected.
- Full source-profile paths and media history are excluded from telemetry.

## Cross-store commit behavior

Settings currently use RedCrown's two-slot journal while library state uses
SQLite. The importer saves merged endpoints first, commits the SQLite import,
and compensates by restoring the previous settings generation if the SQLite
transaction fails. A process crash between stores can leave only the endpoint
merge committed; rerunning the idempotent import safely completes the library
side. Moving all durable settings into SQLite later would permit one physical
transaction, but is not required to preserve data correctness.

## Tradeoffs

Popcorn's watched records often contain identifiers but no cached title or
poster. RedCrown preserves those identities and counts immediately, then may
enrich them from configured catalog providers later. It does not fabricate
display metadata.

Popcorn's DHT endpoint list is treated as user-selected configuration, not as a
trusted update channel. Importing it requires the same explicit confirmation as
custom endpoints, and RedCrown applies its own URL validation.

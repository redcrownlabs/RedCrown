# Episode playback and torrent file selection

## User requirement

Series and anime details must expose seasons, episodes, and every available
quality. Choosing an episode must always stream that episode, including when
several episodes share one season-pack torrent.

## Design

`catalog.episodes` loads the provider's show-detail payload through the same
ordered endpoint fallback chain used by catalog browsing. The catalog layer
normalizes each episode and keeps the provider's exact `file` path on every
`TorrentOption`.

The renderer sends both the selected torrent source and optional `file_path` to
`playback.prepare`, then polls `playback.status`. The torrent engine resolves
metadata before starting the torrent:

- Movie sources without `file_path` select the largest supported media file.
- Episode sources with `file_path` require an exact match after normalizing
  slash direction and harmless leading path markers.
- A missing requested path is an error. The engine never falls back to another
  media file.
- When librqbit reports an already-managed season pack, RedCrown updates its
  `only_files` selection before returning the playback ticket.

## Invariants

- Episode selection is preserved across the provider, IPC, and torrent layers.
- A source cannot silently stream a different episode.
- Duplicate provider entries that point to the same magnet and file path are
  shown once.
- Catalog endpoint failover behavior remains identical for browse and episode
  requests.

## Tradeoffs

Provider media identifiers are restricted to the identifier characters used by
the compatible API (`A-Z`, `a-z`, digits, `.`, `_`, and `-`). This keeps detail
paths unambiguous and prevents provider data from changing the requested route.

The initial UI loads episode metadata when details open rather than loading it
for every catalog card. This adds one detail request but avoids downloading large
episode collections during browsing.

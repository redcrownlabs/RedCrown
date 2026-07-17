# Popcorn-Compatible Provider Contract

Date: 2026-07-16

RedCrown normalizes compatible provider payloads at the Rust boundary. Provider
JSON does not become part of the renderer contract.

## Accepted variations

- The response may be an array or an object containing `results`, `movies`, or
  `shows`.
- A title identifier may be supplied as `id`, `imdb_id`, or `tvdb_id`.
- `title` and `name` are accepted as display-title fields.
- `year` may be an integer or a numeric string.
- `rating` may be a zero-to-ten number, a numeric string, or an object with a
  zero-to-one-hundred `percentage` field. Percentage ratings are converted to a
  ten-point scale and bounded to the renderer's zero-to-ten invariant.
- Torrents may be keyed directly by quality or grouped first by language and
  then by quality.
- Torrent sources may use `url` or `magnet`; seed counts may use `seed` or
  `seeds`.
- Torrent download size is normalized from an integer or numeric-string `size`
  or `bytes` field into `size_bytes`. The UI displays that exact byte count in
  binary units and says that the size is unknown when providers omit it; size
  is never guessed from a quality label.
- Provider aliases that identify the same source and exact internal file are
  merged instead of selecting the first JSON entry. Optional metadata is
  retained across aliases and the highest reported seeder count wins, making
  normalization independent of object-key ordering.
- Provider `file`, `filename`, `name`, or `title` metadata may supply the
  display-only `file_name`, in that priority order. This lets people inspect a
  release name before choosing it without changing which torrent or internal
  file playback resolves.
- Catalog browsing is always page-based. RedCrown passes the one-based page,
  provider sort, optional genre, and optional keywords to the API instead of
  filtering a fixed first page in the renderer.
- Anime uses the compatible provider's `shows/{page}?anime=1` mode. It remains
  a distinct RedCrown category even though the upstream transport shares the
  shows route.
- A full 50-item response advertises a possible next page. Short responses end
  pagination; renderer-side ID deduplication protects against unstable pages.
- `images.fanart` is normalized as wide backdrop artwork. Known TMDB image URLs
  are upgraded from HTTP to HTTPS for both posters and backdrops.
- Poster and fanart values are optional provider metadata. Empty, relative,
  non-string, malformed, and non-HTTP(S) values are ignored per title instead
  of rejecting the complete catalog page. Relative image paths are not resolved
  against the API endpoint because the provider contract does not define that
  endpoint as an image origin.

Unknown fields remain ignored. Missing optional metadata does not reject an
otherwise usable title. A malformed response root or item collection remains a
provider-contract error rather than being guessed.

## DHT limitation on Windows

The selected stable torrent dependency, `librqbit` 8.1.1, uses a Tokio UDP
socket for DHT. Windows can surface an ICMP port-unreachable response as socket
error 10054. The current upstream DHT worker treats that receive error as
terminal, although the surrounding torrent session and tracker discovery
remain alive.

RedCrown persists librqbit's learned DHT routing table in its stream-cache root.
The file is outside expiring info-hash directories and is not a persistent media
download. This avoids making every launch a cold bootstrap and lets later
sessions reuse known DHT nodes when public bootstrap DNS is unavailable. A new
installation still needs either a reachable bootstrap host or tracker-discovered
peers before it has useful routing state.

RedCrown deliberately does not:

- disable DHT silently;
- suppress the error as if DHT remained healthy;
- patch files inside the Cargo registry;
- switch production code to a release-candidate torrent engine.

The durable resolution is an upstream socket-handling change that disables
`SIO_UDP_CONNRESET` for this UDP socket or treats this Windows receive condition
as recoverable, followed by a stable dependency release and RedCrown
qualification.
# Provider detail contract

Series and anime detail requests use:

`show/{media_id}?locale=en&contentLocale=en&showAll=1`

The response must contain an `episodes` array. Episode `season` and `episode`
values may be JSON numbers or numeric strings. Torrent maps may be keyed by
quality or contain provider-generated numeric aliases; RedCrown uses the
torrent object's own `quality` field when present and removes entries that
repeat the same source and exact file path.

Each episode torrent may provide:

- `url` or `magnet`
- `quality`
- `seed` or `seeds`
- `provider`
- `size` or `bytes`, the total download size in bytes
- `file`, the exact media path inside a multi-file torrent
- `filename`, `name`, or `title`, a fallback release name for presentation

The `file` value is part of playback correctness, not presentation metadata.
RedCrown must pass it through unchanged and reject playback when it does not
match the resolved torrent metadata. `file_name` is intentionally separate and
must never be used to select a file from torrent metadata.

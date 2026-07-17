# Vendored librqbit

Upstream: `https://github.com/ikatson/rqbit`

Upstream crate: `librqbit` 8.1.1

Upstream commit: `559fca8552f64099b39c9284c52fd4d3d9a9169f`

## Why this fork exists

Version 8.1.1 passes no listening port to trackers during list-only magnet
metadata resolution. The tracker client serializes that absence as `port=0`.
Standards-compliant tracker participation requires a usable port, and
Rutracker rejects these announces with HTTP 403.

RedCrown always supplies its bound TCP peer-listener port to trackers. It still
supplies the port to DHT only for active torrents, preserving list-only swarm
presence behavior. Focused tests in `src/session.rs` lock in that separation.
The same separation is present in upstream's 9.0 release candidate, but
RedCrown does not adopt release-candidate torrent engines for stable builds.

The default rqbit peer ID uses arbitrary binary bytes in its random suffix.
Rutracker's HTTP edge rejects those query bytes with 403 even though it accepts
the `rQ` client family. RedCrown retains the truthful rqbit fingerprint and
generates the conventional 12-character alphanumeric Azureus suffix. This is
tracker compatibility, not impersonation of another client.

Version 8.1.1 also sends tracker HTTP requests without a `User-Agent` header.
Rutracker rejects anonymous requests with 403. RedCrown backports upstream 9.0's
behavior and sends librqbit's truthful name and version on its shared HTTP
client.

## Maintenance invariant

Keep this fork aligned with upstream 8.1.1 except for documented changes.
Remove the patch after a stable, qualified librqbit release supplies a nonzero
tracker port during metadata resolution. Do not add RedCrown application logic
to this crate.

## Tradeoff

RedCrown temporarily owns security and compatibility updates for the fork.
This is preferable to announcing a fake port, duplicating BitTorrent metadata
resolution, or introducing separate torrent lifecycles for magnet sources.

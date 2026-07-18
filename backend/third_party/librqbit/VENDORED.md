# Vendored librqbit

Upstream: `https://github.com/ikatson/rqbit`

Upstream crate: `librqbit` 9.0.0-rc.0

Upstream commit: `1fd0818e6efc1b48fd15b07fbc09ac8ad6e524cf`

## Why this version is qualified

RedCrown starts playback from magnets and therefore needs to reach swarms that
only accept uTP. The previously qualified 8.1.1 engine attempted TCP only. A
real-world regression torrent exposed 330 recently usable peer endpoints where
none accepted TCP, while a libtorrent client retrieved metadata and 372,821
bytes in 11 seconds. More DHT nodes or trackers cannot compensate for a missing
peer transport.

Version 9.0 races TCP with uTP and keeps the established rqbit streaming and
selective-file APIs. RedCrown vendors the release candidate instead of tracking
the moving upstream branch. Promotion to a public RedCrown release requires the
deterministic local transfer tests and the external cold-magnet test described
in `docs/torrent-startup.md`.

## Historical compatibility fixes

Version 8.1.1 passes no listening port to trackers during list-only magnet
metadata resolution. The tracker client serializes that absence as `port=0`.
Standards-compliant tracker participation requires a usable port, and
Rutracker rejects these announces with HTTP 403.

RedCrown always supplies its bound TCP peer-listener port to trackers. It still
supplies the port to DHT only for active torrents, preserving list-only swarm
presence behavior. Focused tests in `src/session.rs` lock in that separation.
The same separation is present in the qualified 9.0 source.

The default rqbit peer ID uses arbitrary binary bytes in its random suffix.
Rutracker's HTTP edge rejects those query bytes with 403 even though it accepts
the `rQ` client family. RedCrown retains the truthful rqbit fingerprint and
generates the conventional 12-character alphanumeric Azureus suffix. This is
tracker compatibility, not impersonation of another client.

Version 9.0 sends librqbit's truthful name and version on its shared HTTP
client, which avoids Rutracker's anonymous-client rejection.

Version 8.1.1 applies `AddTorrentOptions::trackers` to torrent-file inputs but
silently ignores the same option during magnet metadata discovery. RedCrown
merges those application-supplied trackers into magnet discovery and locks the
option contract in a focused `src/session.rs` regression test.

RedCrown exposes the exact count of pieces already verified in storage. The
application must distinguish cached availability from pieces downloaded during
the current engine session; combining those counters produces impossible-looking
diagnostics when an expiring stream-cache entry is reused.

RedCrown also separates fast-resume bitfields from persistent torrent
membership. `SessionOptions::fastresume_root` stores only verified-piece state
below `<root>/<info-hash>/`; it is mutually exclusive with session persistence.
This lets RedCrown reuse its expiring stream cache without restoring background
downloads. The cache owns both media and bitfield deletion as one lifecycle.

## Maintenance invariant

Keep this fork aligned with the pinned 9.0.0-rc.0 package except for documented,
tested startup changes. Remove the patches after a stable, qualified librqbit
release supplies equivalent behavior. Do not add RedCrown catalog or UI logic
to this crate.

## Tradeoff

RedCrown temporarily owns qualification and security review for a release
candidate. This is preferable to maintaining an independent uTP implementation
or shipping a torrent engine that cannot reach common peers.

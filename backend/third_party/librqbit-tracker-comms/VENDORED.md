# Vendored librqbit tracker communications

Upstream: `https://github.com/ikatson/rqbit`

Upstream crate: `librqbit-tracker-comms` 3.0.0

Upstream commit: `559fca8552f64099b39c9284c52fd4d3d9a9169f`

## Why this fork exists

Version 3.0.0 replaces an HTTP tracker's existing query when adding BitTorrent
announce parameters. Trackers such as Rutracker use a bare `magnet` query flag
to select the magnet announce endpoint. Dropping that flag returns no usable
peers and prevents magnet metadata discovery.

Version 3.0.0 also requires the optional `complete` and `incomplete` swarm
counts. Rutracker's magnet endpoint returns the required interval and peers but
omits those advisory counts. RedCrown defaults missing counts to zero, matching
upstream 9.0 and the permissive tracker response contract.

RedCrown preserves the original query and appends the announce parameters. The
focused tests in `src/tracker_comms.rs` lock in this behavior. Upstream's 9.0
release candidate contains the same semantic correction, but RedCrown does not
adopt release-candidate torrent engines in its stable dependency set.

## Maintenance invariant

Keep this fork byte-for-byte aligned with upstream 3.0.0 except for documented
changes. Remove the patch when a stable, qualified librqbit release preserves
tracker URL query data. Do not add RedCrown-specific behavior here.

## Tradeoff

RedCrown temporarily owns security and compatibility updates for this small
fork. This is preferred over duplicating tracker protocol logic in application
code or depending on an unstable engine release.

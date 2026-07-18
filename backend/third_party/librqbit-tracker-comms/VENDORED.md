# Vendored librqbit tracker communications

Upstream: `https://github.com/ikatson/rqbit`

Upstream crate: `librqbit-tracker-comms` 9.0.0-rc.0

Upstream commit: `1fd0818e6efc1b48fd15b07fbc09ac8ad6e524cf`

## Why this fork exists

Version 3.0.0 replaces an HTTP tracker's existing query when adding BitTorrent
announce parameters. Trackers such as Rutracker use a bare `magnet` query flag
to select the magnet announce endpoint. Dropping that flag returns no usable
peers and prevents magnet metadata discovery.

Older versions also required the optional `complete` and `incomplete` swarm
counts. Rutracker's magnet endpoint returns the required interval and peers but
omits those advisory counts. RedCrown defaults missing counts to zero, matching
upstream 9.0 and the permissive tracker response contract.

Version 9.0 preserves the original query and accepts missing advisory swarm
counts. The focused tests in `src/tracker_comms.rs` lock in this behavior.

## Maintenance invariant

Keep this fork byte-for-byte aligned with the pinned 9.0.0-rc.0 package except
for documented changes. Remove the patch when a stable, qualified librqbit
release preserves the same contract. Do not add RedCrown-specific behavior
here.

## Tradeoff

RedCrown temporarily owns security and compatibility updates for this small
fork. This is preferred over duplicating tracker protocol logic in application
code or depending on an unstable engine release.

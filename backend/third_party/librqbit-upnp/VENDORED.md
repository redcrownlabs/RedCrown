# Vendored librqbit UPnP

Upstream: `https://github.com/ikatson/rqbit`

Upstream crate: `librqbit-upnp` 9.0.0-rc.0

Upstream commit: `1fd0818e6efc1b48fd15b07fbc09ac8ad6e524cf`

## Why this fork exists

This crate is pinned with the qualified rqbit 9 engine so UPnP types and the
shared dual-stack socket dependency remain coherent. Its `quick-xml` 0.38.4
dependency is outside the vulnerable 0.37 series previously removed by
RedCrown.

## Maintenance invariant

Keep this crate aligned with the pinned 9.0.0-rc.0 package. Do not add RedCrown
application behavior here.

## Tradeoff

RedCrown owns security review for the pinned release candidate until a stable,
qualified rqbit release provides the same transport behavior.

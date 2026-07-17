# Vendored librqbit UPnP

Upstream: `https://github.com/ikatson/rqbit`

Upstream crate: `librqbit-upnp` 1.0.0

Upstream source release: `librqbit-upnp-1.0.0` from crates.io

## Why this fork exists

The stable upstream crate requires `quick-xml` 0.37, which is affected by
RUSTSEC-2026-0194 and RUSTSEC-2026-0195. Both advisories can be triggered while
parsing untrusted XML. UPnP device descriptions are network-provided input, so
RedCrown cannot treat the vulnerable parser as unreachable.

The only newer published `librqbit-upnp` version is coupled to the rqbit 9.0
release candidate. RedCrown keeps the qualified stable torrent engine and
updates this small crate to `quick-xml` 0.41, whose deserialization API remains
compatible with the single `quick_xml::de::from_str` call used here.

## Maintenance invariant

Keep this fork aligned with upstream 1.0.0 except for the documented dependency
upgrade. Remove it when a stable, qualified `librqbit-upnp` release uses a
non-vulnerable XML parser. Do not add RedCrown application behavior to this
crate.

## Tradeoff

RedCrown temporarily owns security updates for this fork. This is preferable to
suppressing exploitable advisories, disabling inbound port mapping, or adopting
an otherwise unqualified release-candidate torrent engine.

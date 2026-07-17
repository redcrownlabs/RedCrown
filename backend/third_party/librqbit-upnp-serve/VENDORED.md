# Vendored `librqbit-upnp-serve`

This directory contains the source of `librqbit-upnp-serve` 1.0.1 from the
Apache-2.0 licensed [rqbit project](https://github.com/ikatson/rqbit).

RedCrown vendors this crate because the published release constrains
`quick-xml` to the vulnerable 0.37 series (RUSTSEC-2026-0194 and
RUSTSEC-2026-0195). The only intentional dependency change is allowing the
compatible 0.41 series. The implementation remains upstream source.

The invariant is that the UPnP media-server behavior and public API remain
identical to upstream 1.0.1 while RedCrown's resolved runtime graph contains no
known vulnerable `quick-xml` release. This small maintenance fork is preferred
to suppressing security advisories; it can be removed after an upstream release
adopts a fixed parser version.

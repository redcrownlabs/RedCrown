# Torrent Engine Decision

Date: 2026-07-16
Status: Selected, qualification ongoing

## Requirement

The engine must support Windows, magnets, DHT, selective files, verified-piece
streaming, seeking, bounded resources, cancellation, and temporary cache reuse.

## Decision

Use `librqbit` behind `redcrown-torrent` and never expose its types elsewhere.

On Windows, use the supported default TLS feature set for both `librqbit` and
the catalog client. This delegates certificate validation and trust-store
integration to Windows Schannel and keeps the release build reproducible with
the normal Rust and Visual Studio toolchain.

## Why

`librqbit` is an embeddable Rust library, uses Tokio, supports DHT and magnets,
and powers rqbit's documented seekable HTTP streaming.

## Invariant

RedCrown code may use only public `librqbit` APIs. If a required behavior is
missing, fix it upstream, maintain a documented tested fork, or replace the
engine. Private-field coupling is prohibited.

Windows-native TLS makes the operating system trust store part of the platform
contract. This is intentional for a Windows desktop application.

### Tracker query preservation backport

`librqbit-tracker-comms` 3.0.0 replaces tracker-specific query data when it
adds announce parameters. That breaks trackers whose endpoint behavior depends
on a query flag, including Rutracker's `?magnet` endpoint, and leaves magnet
metadata discovery with no peers.

RedCrown patches this dependency with a narrow vendored fork that preserves the
original query before appending announce parameters. The fork retains its
upstream license and commit provenance and includes focused regression tests.
This keeps the stable `librqbit` 8.1.1 engine while avoiding duplicated tracker
protocol code in RedCrown. Remove the fork after a stable, qualified upstream
release includes equivalent behavior.

The tracker backport also treats omitted `complete` and `incomplete` swarm
counts as zero. These fields are advisory, Rutracker's magnet endpoint omits
them, and upstream 9.0 applies the same defaults.

RedCrown also binds an inbound peer listener from the IANA dynamic/private port
range. The selected port is included in announces; port zero is not valid for
tracker participation and is rejected by Rutracker. The tradeoff is that the OS
may request firewall permission, while outbound peer connections remain usable
if inbound access is denied.

`librqbit` 8.1.1 suppresses that listener port during list-only metadata
resolution, so RedCrown maintains a second narrow backport in the main engine
crate. Trackers always receive the real bound port, while DHT receives it only
when the torrent is active. This preserves metadata-only swarm behavior and
matches the corrected separation in upstream's 9.0 release candidate without
adopting an unstable engine release.

The same backport keeps rqbit's truthful `rQ` peer fingerprint but limits its
random Azureus-style suffix to alphanumeric bytes. Arbitrary binary suffixes
are valid at the peer protocol layer but Rutracker's HTTP edge rejects their
percent-encoded query representation. A printable random suffix is accepted by
strict trackers and remains unique without impersonating another client.

The backport also identifies librqbit with its real name and version in the
shared HTTP client's `User-Agent`. Version 8.1.1 omitted this header and strict
tracker HTTP edges reject anonymous announces. This matches upstream 9.0.

## Qualification still required

- real incomplete-file playback on Windows;
- seek latency and prioritization;
- cancellation and network-loss behavior;
- disk-full and malformed-input behavior;
- repeated-session soak testing;
- cache reuse and expiration.

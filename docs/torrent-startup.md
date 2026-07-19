# Torrent startup architecture

## Requirement

RedCrown should begin transferring a healthy public magnet in less than ten
seconds when the local network permits it. Startup is measured from the start
of metadata resolution to the first verified media bytes. The UI reports
metadata discovery and transfer as separate phases, but both count toward this
latency target.

This is a performance target, not a promise that an offline swarm can transfer.
RedCrown must fail honestly when no peer has metadata or requested pieces.

## Engine boundary

RedCrown uses a pinned, reviewed fork of rqbit rather than implementing the
BitTorrent protocols itself. The fork supplies DHT, trackers, TCP, uTP, peer
wire protocol, piece verification, selective files, and streaming. RedCrown
owns startup policy, cache lifetime, media selection, diagnostics, and the
loopback playback endpoint.

The pinned rqbit 9.0.0 release candidate is intentional. The earlier 8.1.1
engine supported only outgoing TCP connections. A captured libtorrent session
for the regression swarm downloaded 372,821 bytes in 11 seconds, while none of
its 330 saved peer endpoints subsequently accepted TCP. Version 9 races TCP and
uTP for each peer and preserves rqbit's selective streaming API.

## Windows UDP invariant

Windows can report ICMP UDP `PORT_UNREACHABLE` or `NET_UNREACHABLE` responses
as `WSAECONNRESET`/10054 or `WSAENETRESET`/10052 on a later receive. Stale DHT
and uTP nodes are normal, so these responses must not terminate a shared receive
loop.

The vendored dual-stack socket crate applies `SIO_UDP_CONNRESET = FALSE` and
`SIO_UDP_NETRESET = FALSE` to every Windows UDP socket before binding it.
Failure to apply either control fails socket initialization. Continuing without
them would create an engine that appears healthy and then silently loses
discovery or piece transfer after ordinary network input.

## Metadata scheduling

Metadata resolution uses these bounds:

- 256 simultaneous metadata handshakes;
- a four-second connection timeout;
- a five-second metadata read/write timeout;
- at most 4,096 queued candidate addresses;
- newest candidates first when a handshake slot becomes available.

The queue stays bounded because public tracker lists and DHT can return many
thousands of stale or poisoned addresses. Favoring recent answers prevents
fresh DHT results from waiting behind an unbounded FIFO of timed-out peers. The
peer that supplied valid metadata is placed first in the initial transfer peer
list because it has already completed a compatible handshake.

Normal piece-transfer connections retain rqbit's less aggressive defaults.
Metadata is small and benefits from rapid candidate churn; video transfer must
tolerate slower but useful peers.

## Diagnostics semantics

Stream-cache bytes remain available across engine sessions until the configured
expiration or size policy removes them. Diagnostics therefore reports local
availability separately from current-session network transfer. The overall and
piece progress bars represent hash-verified data available from storage;
"downloaded this session" counters represent only data verified since the
current engine started. Current speeds are instantaneous estimators and return
to zero after the selected content is locally complete.

Diagnostics polling is serial and self-healing. A transient IPC failure remains
visible but cannot permanently stop subsequent refreshes.

## Cached playback startup

Verified-piece bitfields live inside the same info-hash directory as cached
media. Reopening a cache entry uses rqbit's sampled fast-resume validation
instead of hashing the complete selected file. Torrent membership remains
session-only, and deleting or expiring an entry removes its media and bitfield
together. A failed validation clears the bitfield and falls back to a complete
hash check, preserving BitTorrent integrity.

## Playback timeline invariant

The loopback media bridge copies compatible H.264 video to keep startup and
seeking responsive, while transcoding unsupported audio to AAC. HEVC video is
converted to H.264. After a seek, every emitted track must retain the same
relative source timeline. Accurate input seeking cannot satisfy that invariant
when H.264 video is copied: FFmpeg keeps video from the preceding keyframe but
discards the corresponding decoded audio pre-roll. It also makes HEVC seeks
decode and discard the complete keyframe pre-roll before emitting anything,
which produces multi-second stalls on CPU conversion.

All seeked playback therefore places `-noaccurate_seek` before the input `-ss`
and uses asynchronous audio resampling for small timestamp drift. Video and
audio begin at the same preceding keyframe boundary without decoding discarded
pre-roll. The accepted tradeoff is that a seek may resume slightly before the
requested time by at most one source GOP.

The bridge qualifies Windows Media Foundation with a bounded synthetic encode
when the torrent engine starts. A successful probe selects `h264_mf` for lower
interactive SDR HEVC conversion latency. HDR conversion retains
`libopenh264`, whose software-frame input is compatible with the qualified
tone-map filters. Missing media components, unsupported platforms, probe
errors, and probe timeouts also retain that bundled fallback. Playback allows
a five-second input burst before returning to its 1.1x rate limit, providing
an initial playable fragment without permitting the bridge to race through the
complete source.

The media bridge integration test uses a deterministic long-GOP video with
E-AC-3 audio, seeks between keyframes, and compares the first advancing video
and audio packet timestamps. It also exercises the CPU H.264 conversion branch
and verifies the resulting codecs and timestamps. Unit tests separately lock
in FFmpeg's order-sensitive input options, encoder fallback policy, and HEVC
bitrate policy.

## Verification

Deterministic tests lock in:

- TCP/uTP listener configuration;
- bounded newest-first metadata candidates;
- survival of Windows UDP unreachable responses without terminating DHT or uTP;
- local seed-to-client piece verification and loopback streaming;
- DHT persistence and temporary media-cache behavior.

The external acceptance test is intentionally ignored in ordinary CI because
public swarm state is not deterministic. Run it explicitly:

```powershell
.\scripts\test-magnet.ps1 `
  -Magnet 'magnet:?xt=urn:btih:<40-character-info-hash>' `
  -MetadataTimeoutSeconds 10 `
  -TransferTimeoutSeconds 30
```

Ordinary tests bind their peer listeners to loopback so routine builds do not
request a Windows Firewall exception. The explicit external acceptance test and
the production application bind a public peer listener because uTP connectivity
depends on receiving peer datagrams. Windows may request firewall approval for
those executables; RedCrown does not install a broad or privileged exception.

On 2026-07-18, the regression hash that previously stalled passed twice under
the ten-second target. After both Windows reset controls were applied, metadata
resolved in 2.52 seconds and the first media bytes arrived at 2.63 seconds of
measured torrent startup. Compilation time is excluded from these engine timing
measurements.

## Tradeoffs

RedCrown temporarily owns review and qualification of an rqbit release
candidate plus focused metadata scheduling and Windows socket patches. This
adds maintenance work, but it is narrower and safer than implementing uTP or
DHT independently. The forks must remain close to upstream; RedCrown catalog
and UI behavior must not enter them.

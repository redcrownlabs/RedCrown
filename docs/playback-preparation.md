# Playback preparation and startup buffering

## User requirement

Watch must react immediately and explain what is happening while torrent
metadata and media bytes are being acquired. The metadata phase is named
explicitly; after metadata resolves, the progress meter and fixed transfer
fields carry the ordinary download phase without another repetitive status
sentence. Their reserved grid dimensions prevent live values from shifting the
layout.

## Design

Playback preparation is an asynchronous backend session:

1. `playback.prepare` creates a preparation identifier and returns in the
   `resolving_metadata` stage.
2. The renderer polls `playback.status` without overlapping requests.
3. Once metadata is available, the engine selects the exact requested file and
   moves to `buffering`.
4. The engine waits for librqbit's managed torrent to finish checksum and
   storage initialization. Opening a stream before this barrier is an error and
   must never trigger torrent cleanup.
5. The engine reads the beginning of that file through librqbit. This both
   prioritizes the bytes needed for playback and validates that the stream is
   readable.
6. At the startup threshold, FFprobe reads the selected container's real
   duration and track manifest through the protected loopback stream.
7. The stage becomes `ready`; Chromium receives a fragmented MP4 stream from
   the media bridge. Compatible video is copied without quality loss and the
   selected audio track is converted to AAC. Transfer statistics continue to
   update while playback is open.

The startup threshold is one percent of the selected file, bounded to a minimum
of 8 MiB and a maximum of 32 MiB. This avoids starting a large title from a
token buffer while also avoiding an excessive wait for very large files.

`playback.cancel` aborts metadata or buffering work and releases any torrent and
cache lease already created by that preparation. Starting another preparation
first cancels the previous one.

## Invariants

- Desktop IPC never waits for peer discovery, metadata, or startup buffering.
- At most one playback preparation is active in the current desktop session.
- A cancelled or failed preparation cannot leave an active torrent behind.
- `ready` means the configured startup prefix was successfully read, not merely
  that torrent metadata exists.
- Startup buffering cannot open a file stream before librqbit reports that the
  managed torrent has initialized.
- Download percentage is based on the selected media file, not an entire
  multi-file torrent.
- Episode file selection remains exact; preparation never falls back to a
  different file.

## Media bridge and track selection

The selected FFmpeg/FFprobe build is pinned and checksum-verified as documented
in `ffmpeg-distribution.md`. RedCrown never searches the machine `PATH`.

FFprobe track indexes are retained in an in-memory manifest keyed by torrent and
file identifier. Audio and subtitle requests are accepted only when their index
exists in that manifest. The original tokenized range stream remains internal
to the bridge. Public playback responses are non-cacheable and process stderr
is bounded before logging.

Audio selection restarts the compatibility stream at the current presentation
time with the requested track. Seeking uses the same restart mechanism because
a live fragmented output cannot satisfy arbitrary byte ranges. H.264 video is
copied without generation loss. HEVC video is converted to H.264 by the bundled
CPU OpenH264 encoder because Chromium HEVC support is not dependable across the
supported Windows installations. Audio is converted to AAC at 192 kbit/s.
FFmpeg input is rate-limited to a small amount above real time so conversion
cannot race ahead and turn streaming into an accidental persistent download.
A bounded five-second initial burst reduces first-frame and seek latency before
that sustained limit applies.

The HEVC conversion target is derived from the source video bitrate when
FFprobe exposes it, with a 1.5x allowance for H.264's lower compression
efficiency and a 2–40 Mbit/s safety range. Resolution-based targets from 5 to
24 Mbit/s are used when bitrate metadata is absent. Conversion always uses
8-bit 4:2:0 H.264 High profile for Chromium compatibility and a bounded GOP for
responsive fragmented playback. PQ and HLG HDR transfers are normalized,
tone-mapped with FFmpeg's deterministic Mobius filter, and emitted as BT.709
SDR instead of being truncated into incorrect 8-bit color. The source file and
torrent cache remain untouched; only the temporary loopback presentation
stream is converted.

Embedded subtitle selection starts a separate, rate-limited WebVTT conversion
at the same presentation time. Chromium receives it through a native `<track>`
element. The container's default audio disposition is honored initially;
subtitles are off initially and are never chosen from filename guesses. Track
language and title metadata is treated as optional: when a muxer strips both,
the player preserves every distinct stream and labels it `Subtitle N` rather
than presenting the subtitle codec as if it were a language.

## Tradeoffs

The startup percentage describes downloaded availability, not sequential
playback duration: BitTorrent pieces can arrive out of order. The explicit
prefix read is therefore the readiness gate, while librqbit's per-file progress
is used for the user-facing percentage and transfer telemetry.

Polling uses one chained request at a time. A push channel could reduce status
traffic later, but polling keeps the IPC contract simple and prevents stale or
overlapping requests during cancellation.

The compatibility bridge optimizes the common H.264 torrent path by copying
video and explicitly qualifies HEVC through deterministic CPU conversion.
HEVC conversion costs CPU and is necessarily lossy, but avoids hardware- and
driver-dependent behavior. Other source video codecs remain rejected until
their decode, encode, seek, synchronization, and packaged-runtime behavior are
qualified deliberately.

## Diagnostics

`playback.diagnostics` exposes a RedCrown-owned snapshot for the active
preparation. It includes engine state, transfer counters, configured trackers,
aggregate peer states, verified-piece counts, and DHT routing health. The
diagnostics screen polls this snapshot once per second without overlapping
requests. When the preparation source is a magnet URI, the original URI is
retained in memory and shown with a copy action; HTTP torrent sources are not
misrepresented as magnet links.

librqbit 8.1.1 does not expose whether each remote peer owns every piece through
its stable aggregate API. Seeder count is therefore reported as unavailable,
not inferred from catalog seed estimates or fabricated from connected peers.
Configured tracker URLs are observable, but per-tracker announce results are not
available through the same API version.

## Engine-level verification

The `redcrown-torrent` local transfer test runs independently of Electron and the
catalog provider. It creates deterministic media and torrent metadata, starts a
localhost seed session, starts a separate RedCrown client, waits for
initialization, downloads and verifies every piece, reads the startup buffer,
fetches the tokenized loopback HTTP stream, and compares every output byte with
the source.

Run it from the repository root with `./scripts/test-torrent.ps1`.

For a real public swarm, use
`./scripts/test-magnet.ps1 -Magnet '<magnet URI>'`. This ignored network
qualification test imports the configured tracker list, resolves metadata,
selects a supported media file, and verifies a 256 KiB loopback range outside
Electron. The magnet is supplied only through the process environment and is
not stored in the repository.

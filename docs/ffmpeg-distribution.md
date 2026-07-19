# FFmpeg distribution and media compatibility

Date: 2026-07-17

## Requirement

Torrent media commonly contains E-AC-3, AC-3, DTS, and multiple audio or
subtitle tracks. Chromium cannot provide reliable native selection and decoding
for that set. RedCrown therefore owns a media-compatibility boundary instead of
silently playing video without audio or depending on software installed on the
user's machine.

## Pinned toolchain

Development uses the BtbN FFmpeg 8.1 shared LGPL build identified by
`ffmpeg-n8.1.2-22-g94138f6973-win64-lgpl-shared-8.1`, from the immutable GitHub
release `autobuild-2026-07-17-13-22`. `scripts/ensure-ffmpeg.ps1` downloads the
archive and verifies SHA-256
`fcbf0f5c58fec3e516e35ba26d81bc6cbaea09dde76bffd151fa93c0316b0b50`
before extraction. A checksum mismatch is fatal; mutable or unverified media
executables must never run.

Downloaded tools live under the ignored `.redcrown/tools` development folder.
A packaged build must place the complete shared-build directory in its
resources and pass the bundled `ffmpeg.exe` and `ffprobe.exe` paths to the Rust
backend. Falling back to `PATH` is prohibited because it would make codec
behavior machine-dependent.

## Licensing invariant

The selected build is marked LGPL and shared. Distribution must include the
corresponding FFmpeg notices, build configuration, and corresponding source as
required by the FFmpeg legal checklist. RedCrown's future installer work is not
complete until those materials are present and the packaged executable paths
are integration-tested.

## Media invariant

FFprobe is the source of truth for track indexes, codecs, language tags,
titles, dispositions, and duration. The renderer never infers tracks from a
filename. FFmpeg receives only indexes that were returned by that manifest.
Audio selected by the user is converted to AAC while compatible H.264 video is
stream-copied, avoiding needless video quality loss and CPU cost. HEVC is
decoded and converted to H.264 by the packaged CPU OpenH264 encoder so playback
does not depend on optional Windows codecs or GPU drivers. PQ and HLG sources
are tone-mapped to BT.709 SDR by the packaged zscale and tonemap filters before
encoding. Subtitle tracks are converted to WebVTT for Chromium's native
text-track interface. Both playback and subtitle inputs are rate-limited so
track handling does not eagerly consume the complete torrent.

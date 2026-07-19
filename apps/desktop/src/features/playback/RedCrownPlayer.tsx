import { useEffect, useEffectEvent, useRef, useState } from "react";
import type { CSSProperties, MouseEvent } from "react";

import type { MediaItem, MediaTrack, PlaybackStatus } from "../../shared/contract.generated";
import { formatBytes, formatDownloadSpeed } from "./playback-model";
import {
  clampedSeekTime,
  formatPlaybackTime,
  mediaPercent,
  playbackStreamUrl,
  subtitleStreamUrl,
  trackDisplayDetail,
  trackDisplayLabel,
} from "./player-model";

const SEEK_RESTART_DELAY_MS = 120;

type PlayerIconName =
  | "back"
  | "enter-fullscreen"
  | "exit-fullscreen"
  | "pause"
  | "play"
  | "volume"
  | "volume-muted";

function PlayerIcon({ name }: { name: PlayerIconName }) {
  const paths = {
    back: <path d="m15 18-6-6 6-6" />,
    "enter-fullscreen": <><path d="M8 3H3v5M16 3h5v5M8 21H3v-5M16 21h5v-5" /></>,
    "exit-fullscreen": <><path d="M3 8h5V3M21 8h-5V3M3 16h5v5M21 16h-5v5" /></>,
    pause: <><path d="M8 5v14M16 5v14" /></>,
    play: <path d="m8 5 11 7-11 7z" />,
    volume: <><path d="M5 9v6h4l5 4V5L9 9H5Z" /><path d="M17 9a4 4 0 0 1 0 6M19.5 6.5a8 8 0 0 1 0 11" /></>,
    "volume-muted": <><path d="M5 9v6h4l5 4V5L9 9H5Z" /><path d="m18 9 4 6M22 9l-4 6" /></>,
  };
  return <svg aria-hidden="true" viewBox="0 0 24 24">{paths[name]}</svg>;
}

export function RedCrownPlayer({
  item,
  status,
  onClose,
}: {
  item: MediaItem;
  status: PlaybackStatus;
  onClose: () => void;
}) {
  const stageRef = useRef<HTMLDivElement>(null);
  const videoRef = useRef<HTMLVideoElement>(null);
  const ticket = status.ticket;
  const controlsTimer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);
  const seekTimer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);
  const pendingSeek = useRef<number | undefined>(undefined);
  const [paused, setPaused] = useState(true);
  const [buffering, setBuffering] = useState(true);
  const [controlsVisible, setControlsVisible] = useState(true);
  const [currentTime, setCurrentTime] = useState(0);
  const [duration, setDuration] = useState(ticket?.duration_seconds ?? 0);
  const [bufferedUntil, setBufferedUntil] = useState(0);
  const [volume, setVolume] = useState(1);
  const [muted, setMuted] = useState(false);
  const [fullscreen, setFullscreen] = useState(false);
  const [mediaError, setMediaError] = useState<string>();
  const [streamStart, setStreamStart] = useState(0);
  const [seekPreview, setSeekPreview] = useState<number>();
  const [selectedAudio, setSelectedAudio] = useState(
    ticket?.audio_tracks.find((track) => track.is_default)?.id
      ?? ticket?.audio_tracks[0]?.id,
  );
  const [selectedSubtitle, setSelectedSubtitle] = useState<number>();

  function clearControlsTimer() {
    if (controlsTimer.current) clearTimeout(controlsTimer.current);
  }

  function clearSeekTimer() {
    if (seekTimer.current) clearTimeout(seekTimer.current);
    seekTimer.current = undefined;
    pendingSeek.current = undefined;
  }

  function revealControls() {
    clearControlsTimer();
    setControlsVisible(true);
    if (!videoRef.current?.paused) {
      controlsTimer.current = setTimeout(() => setControlsVisible(false), 2_800);
    }
  }

  useEffect(() => {
    const onFullscreenChange = () => setFullscreen(document.fullscreenElement === stageRef.current);
    document.addEventListener("fullscreenchange", onFullscreenChange);
    return () => {
      clearControlsTimer();
      clearSeekTimer();
      document.removeEventListener("fullscreenchange", onFullscreenChange);
    };
  }, []);

  async function togglePlayback() {
    const video = videoRef.current;
    if (!video) return;
    revealControls();
    if (video.paused) {
      try {
        await video.play();
      } catch {
        setMediaError("Playback could not start. Try again after more data has buffered.");
      }
    } else {
      video.pause();
    }
  }

  function restartStreamAt(value: number) {
    const target = duration > 0
      ? Math.min(Math.max(0, value), Math.max(0, duration - 0.01))
      : Math.max(0, value);
    clearSeekTimer();
    setStreamStart(target);
    setCurrentTime(target);
    setBufferedUntil(target);
    setSeekPreview(undefined);
    setBuffering(true);
    setMediaError(undefined);
  }

  function seekTo(value: number) {
    const target = clampedSeekTime(value, 0, duration);
    const video = videoRef.current;
    const localTarget = target - streamStart;
    if (video && localTarget >= 0) {
      for (let index = 0; index < video.buffered.length; index += 1) {
        if (localTarget >= video.buffered.start(index) && localTarget <= video.buffered.end(index)) {
          clearSeekTimer();
          video.currentTime = localTarget;
          setCurrentTime(target);
          setSeekPreview(undefined);
          return;
        }
      }
    }
    restartStreamAt(target);
  }

  function scheduleSeekBy(offset: number) {
    const target = clampedSeekTime(pendingSeek.current ?? currentTime, offset, duration);
    if (seekTimer.current) clearTimeout(seekTimer.current);
    pendingSeek.current = target;
    setSeekPreview(target);
    seekTimer.current = setTimeout(() => seekTo(target), SEEK_RESTART_DELAY_MS);
  }

  function changeVolume(value: number) {
    const video = videoRef.current;
    if (!video) return;
    video.volume = value;
    video.muted = value === 0;
    setVolume(value);
    setMuted(value === 0);
  }

  function toggleMute() {
    const video = videoRef.current;
    if (!video) return;
    video.muted = !video.muted;
    setMuted(video.muted);
  }

  async function toggleFullscreen() {
    if (document.fullscreenElement) {
      await document.exitFullscreen();
    } else {
      await stageRef.current?.requestFullscreen();
    }
  }

  const handleKeyboard = useEffectEvent((event: globalThis.KeyboardEvent) => {
    const target = event.target;
    if (
      target instanceof HTMLInputElement
      || target instanceof HTMLTextAreaElement
      || target instanceof HTMLSelectElement
      || (target instanceof HTMLElement && target.isContentEditable)
    ) return;
    if (event.key === " " && target instanceof HTMLButtonElement) return;
    switch (event.key.toLowerCase()) {
      case " ":
      case "k":
        event.preventDefault();
        void togglePlayback();
        break;
      case "arrowleft":
        event.preventDefault();
        scheduleSeekBy(-10);
        revealControls();
        break;
      case "arrowright":
        event.preventDefault();
        scheduleSeekBy(10);
        revealControls();
        break;
      case "m":
        toggleMute();
        revealControls();
        break;
      case "f":
        void toggleFullscreen();
        break;
      case "escape":
        if (!document.fullscreenElement) onClose();
        break;
    }
  });

  useEffect(() => {
    document.addEventListener("keydown", handleKeyboard);
    return () => document.removeEventListener("keydown", handleKeyboard);
  }, []);

  function updateBuffered() {
    const video = videoRef.current;
    if (!video || video.buffered.length === 0) {
      setBufferedUntil(0);
      return;
    }
    setBufferedUntil(streamStart + video.buffered.end(video.buffered.length - 1));
  }

  function handleVideoClick(event: MouseEvent<HTMLVideoElement>) {
    if (event.detail === 1) void togglePlayback();
  }

  const positionPercent = mediaPercent(currentTime, duration);
  const bufferedPercent = mediaPercent(bufferedUntil, duration);
  const effectiveVolume = muted ? 0 : volume;
  const playbackSource = ticket
    ? playbackStreamUrl(ticket.playback_url, selectedAudio, streamStart)
    : undefined;
  const stageStyle = {
    "--player-position": `${positionPercent}%`,
    "--player-buffered": `${bufferedPercent}%`,
  } as CSSProperties;

  return (
    <section className="redcrown-player" aria-label={`Player for ${item.title}`}>
      <div
        ref={stageRef}
        className={`redcrown-player-stage${controlsVisible || paused ? " controls-visible" : ""}`}
        style={stageStyle}
        role="group"
        aria-label="Video playback controls"
        tabIndex={0}
        onMouseMove={revealControls}
        onMouseLeave={() => {
          if (!paused) setControlsVisible(false);
        }}
        onDoubleClick={() => void toggleFullscreen()}
      >
        <video
          ref={videoRef}
          src={playbackSource}
          poster={item.backdrop_url}
          autoPlay
          playsInline
          width="1920"
          height="1080"
          onClick={handleVideoClick}
          onCanPlay={() => {
            setBuffering(false);
            setMediaError(undefined);
          }}
          onPlaying={() => {
            setPaused(false);
            setBuffering(false);
            revealControls();
          }}
          onPause={() => {
            setPaused(true);
            setControlsVisible(true);
            clearControlsTimer();
          }}
          onWaiting={() => setBuffering(true)}
          onStalled={() => setBuffering(true)}
          onDurationChange={(event) => {
            if (ticket?.duration_seconds != null) return;
            setDuration(Number.isFinite(event.currentTarget.duration)
              ? streamStart + event.currentTarget.duration
              : 0);
          }}
          onTimeUpdate={(event) => setCurrentTime(streamStart + event.currentTarget.currentTime)}
          onProgress={updateBuffered}
          onVolumeChange={(event) => {
            setVolume(event.currentTarget.volume);
            setMuted(event.currentTarget.muted);
          }}
          onError={(event) => {
            setBuffering(false);
            setMediaError(
              event.currentTarget.error?.message
                || "Electron could not decode this media format.",
            );
          }}
        >
          {ticket?.subtitle_tracks
            .filter((track) => track.id === selectedSubtitle && track.stream_url)
            .map((track) => (
              <track
                key={track.id}
                kind="subtitles"
                src={subtitleStreamUrl(track.stream_url!, streamStart)}
                srcLang={track.language ?? "und"}
                label={trackDisplayLabel(
                  track,
                  `Subtitle ${ticket.subtitle_tracks.indexOf(track) + 1}`,
                )}
                default
              />
            ))}
        </video>

        <div className="player-cinematic-shade" aria-hidden="true" />

        <header className="player-topbar">
          <button className="back-button player-back" onClick={onClose} type="button">
            <PlayerIcon name="back" />
            <span>Back to title</span>
          </button>
          <div className="player-title">
            <p>Now playing</p>
            <h1>{item.title}</h1>
            <span>{status.ticket?.file_name}</span>
          </div>
          <div className="player-network" aria-label="Torrent transfer status">
            <span>{formatDownloadSpeed(status.download_mib_per_second)}</span>
            <span>{formatBytes(status.downloaded_bytes)} ready</span>
            <span>{status.connected_peers} peers</span>
          </div>
        </header>

        {buffering && !mediaError && (
          <div className="player-buffering" aria-label="Buffering video">
            <span aria-hidden="true" />
            <p>Buffering stream</p>
          </div>
        )}

        {!buffering && paused && !mediaError && (
          <button className="player-center-play" onClick={() => void togglePlayback()} type="button" aria-label="Play">
            <PlayerIcon name="play" />
          </button>
        )}

        {mediaError && (
          <div className="player-media-error" role="alert">
            <strong>Playback couldn’t continue</strong>
            <p>{mediaError}</p>
            <p>The selected media bridge or track could not be decoded.</p>
          </div>
        )}

        <div className="player-controls" onFocus={revealControls}>
          <label className="player-timeline">
            <span className="visually-hidden">Playback position</span>
            <input
              type="range"
              min={0}
              max={duration || 0}
              step="any"
              value={Math.min(seekPreview ?? currentTime, duration || 0)}
              onChange={(event) => setSeekPreview(event.currentTarget.valueAsNumber)}
              onPointerUp={(event) => seekTo(event.currentTarget.valueAsNumber)}
              onKeyUp={(event) => {
                if (["ArrowLeft", "ArrowRight", "Home", "End", "PageUp", "PageDown"].includes(event.key)) {
                  seekTo(event.currentTarget.valueAsNumber);
                }
              }}
              aria-valuetext={`${formatPlaybackTime(currentTime)} of ${formatPlaybackTime(duration)}`}
            />
          </label>

          <div className="player-control-row">
            <button className="player-icon-button player-play-toggle" onClick={() => void togglePlayback()} type="button" aria-label={paused ? "Play" : "Pause"}>
              <PlayerIcon name={paused ? "play" : "pause"} />
            </button>
            <span className="player-time">{formatPlaybackTime(currentTime)} <i>/</i> {formatPlaybackTime(duration)}</span>

            <div className="player-volume">
              <button className="player-icon-button" onClick={toggleMute} type="button" aria-label={muted ? "Unmute" : "Mute"} aria-pressed={muted}>
                <PlayerIcon name={effectiveVolume === 0 ? "volume-muted" : "volume"} />
              </button>
              <label>
                <span className="visually-hidden">Volume</span>
                <input
                  type="range"
                  min={0}
                  max={1}
                  step={0.05}
                  value={effectiveVolume}
                  onChange={(event) => changeVolume(event.currentTarget.valueAsNumber)}
                />
              </label>
            </div>

            {ticket && (
              <div className="player-track-controls">
                <TrackPicker
                  label="Audio"
                  tracks={ticket.audio_tracks}
                  selected={selectedAudio}
                  onSelect={(track) => {
                    if (track === selectedAudio) return;
                    setSelectedAudio(track);
                    restartStreamAt(currentTime);
                  }}
                />
                <TrackPicker
                  label="Subtitles"
                  tracks={ticket.subtitle_tracks}
                  selected={selectedSubtitle}
                  allowOff
                  onSelect={(track) => {
                    if (track === selectedSubtitle) return;
                    setSelectedSubtitle(track);
                    if (track != null) restartStreamAt(currentTime);
                  }}
                />
              </div>
            )}

            <span className="player-shortcuts" aria-hidden="true">← → seek&nbsp;&nbsp; M mute&nbsp;&nbsp; F fullscreen</span>
            <button className="player-icon-button" onClick={() => void toggleFullscreen()} type="button" aria-label={fullscreen ? "Exit fullscreen" : "Enter fullscreen"}>
              <PlayerIcon name={fullscreen ? "exit-fullscreen" : "enter-fullscreen"} />
            </button>
          </div>
        </div>
      </div>
    </section>
  );
}

function TrackPicker({
  label,
  tracks,
  selected,
  allowOff = false,
  onSelect,
}: {
  label: string;
  tracks: MediaTrack[];
  selected?: number;
  allowOff?: boolean;
  onSelect: (track?: number) => void;
}) {
  const detailsRef = useRef<HTMLDetailsElement>(null);
  const selectedTrack = tracks.find((track) => track.id === selected);
  const fallbackNoun = label === "Subtitles" ? "Subtitle" : label;
  const selectedIndex = selectedTrack ? tracks.indexOf(selectedTrack) : -1;
  if (!tracks.length) return null;

  function choose(track?: number) {
    onSelect(track);
    detailsRef.current?.removeAttribute("open");
  }

  return (
    <details className="player-track-picker" ref={detailsRef}>
      <summary>{label}<span>{selectedTrack
        ? trackDisplayLabel(selectedTrack, `${fallbackNoun} ${selectedIndex + 1}`)
        : "Off"}</span></summary>
      <div className="player-track-list">
        {allowOff && (
          <button className={selected == null ? "active" : ""} type="button" aria-pressed={selected == null} onClick={() => choose()}>
            <span>Off</span>
          </button>
        )}
        {tracks.map((track, index) => (
          <button className={selected === track.id ? "active" : ""} type="button" aria-pressed={selected === track.id} key={track.id} onClick={() => choose(track.id)}>
            <span>{trackDisplayLabel(track, `${fallbackNoun} ${index + 1}`)}</span>
            <small>{trackDisplayDetail(track)}</small>
          </button>
        ))}
      </div>
    </details>
  );
}

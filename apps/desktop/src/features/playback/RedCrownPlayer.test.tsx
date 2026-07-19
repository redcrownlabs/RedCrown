/** @vitest-environment jsdom */

import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type { MediaItem, PlaybackStatus } from "../../shared/contract.generated";
import { RedCrownPlayer } from "./RedCrownPlayer";

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });

const item = {
  id: "movie-1",
  title: "Playback fixture",
  synopsis: "",
  kind: "movie",
  genres: [],
  torrents: [],
} satisfies MediaItem;

const status = {
  preparation_id: "preparation-1",
  stage: "ready",
  downloaded_bytes: 1_000,
  total_bytes: 2_000,
  download_mib_per_second: 1,
  connected_peers: 1,
  ticket: {
    torrent_id: 1,
    file_id: 2,
    file_name: "fixture.mkv",
    file_length: 2_000,
    stream_url: "http://127.0.0.1:4321/stream?token=test",
    playback_url: "http://127.0.0.1:4321/playback?token=test",
    duration_seconds: 120,
    audio_tracks: [{
      id: 1,
      codec: "eac3",
      language: "eng",
      is_default: true,
      is_forced: false,
    }],
    subtitle_tracks: [],
  },
} satisfies PlaybackStatus;

describe("RedCrownPlayer keyboard shortcuts", () => {
  let container: HTMLDivElement;
  let root: Root;

  beforeEach(() => {
    container = document.createElement("div");
    document.body.append(container);
    root = createRoot(container);
    act(() => {
      root.render(<RedCrownPlayer item={item} status={status} onClose={vi.fn()} />);
    });
  });

  afterEach(() => {
    act(() => root.unmount());
    container.remove();
    vi.useRealTimers();
  });

  it("seeks from a document-level arrow key even when the stage is not focused", () => {
    vi.useFakeTimers();
    const video = container.querySelector("video");
    expect(video).not.toBeNull();

    act(() => {
      document.dispatchEvent(new KeyboardEvent("keydown", {
        key: "ArrowRight",
        bubbles: true,
      }));
      vi.advanceTimersByTime(120);
    });

    expect(video?.src).toContain("start=10.000");
    vi.useRealTimers();
  });

  it("coalesces repeated keyboard seeks into one stream restart", () => {
    vi.useFakeTimers();
    const video = container.querySelector("video");

    act(() => {
      document.dispatchEvent(new KeyboardEvent("keydown", { key: "ArrowRight", bubbles: true }));
      document.dispatchEvent(new KeyboardEvent("keydown", { key: "ArrowRight", bubbles: true }));
      vi.advanceTimersByTime(120);
    });

    expect(video?.src).toContain("start=20.000");
    vi.useRealTimers();
  });

  it("seeks inside buffered media without restarting the bridge", () => {
    vi.useFakeTimers();
    const video = container.querySelector("video");
    expect(video).not.toBeNull();
    Object.defineProperty(video, "buffered", {
      configurable: true,
      value: {
        length: 1,
        start: () => 0,
        end: () => 30,
      },
    });

    act(() => {
      document.dispatchEvent(new KeyboardEvent("keydown", { key: "ArrowRight", bubbles: true }));
      vi.advanceTimersByTime(120);
    });

    expect(video?.src).not.toContain("start=");
    expect(video?.currentTime).toBe(10);
    vi.useRealTimers();
  });

  it("mutes after a player button retained focus", () => {
    const video = container.querySelector("video");
    const mute = container.querySelector<HTMLButtonElement>('button[aria-label="Mute"]');
    expect(video).not.toBeNull();
    expect(mute).not.toBeNull();
    mute?.focus();

    act(() => {
      mute?.dispatchEvent(new KeyboardEvent("keydown", { key: "m", bubbles: true }));
    });

    expect(video?.muted).toBe(true);
    expect(container.querySelector('button[aria-label="Unmute"]')).not.toBeNull();
  });

  it("leaves arrow keys to an editable range control", () => {
    const video = container.querySelector("video");
    const timeline = container.querySelector<HTMLInputElement>(".player-timeline input");
    expect(video).not.toBeNull();
    expect(timeline).not.toBeNull();
    timeline?.focus();

    act(() => {
      timeline?.dispatchEvent(new KeyboardEvent("keydown", {
        key: "ArrowRight",
        bubbles: true,
      }));
    });

    expect(video?.src).not.toContain("start=");
  });
});

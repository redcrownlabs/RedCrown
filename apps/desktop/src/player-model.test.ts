import { describe, expect, it } from "vitest";

import {
  clampedSeekTime,
  formatPlaybackTime,
  mediaPercent,
  playbackStreamUrl,
  subtitleStreamUrl,
} from "./player-model";

describe("player formatting and seeking", () => {
  it("formats short and long playback times", () => {
    expect(formatPlaybackTime(65.9)).toBe("1:05");
    expect(formatPlaybackTime(3_661)).toBe("1:01:01");
    expect(formatPlaybackTime(Number.NaN)).toBe("0:00");
  });

  it("bounds media progress", () => {
    expect(mediaPercent(25, 100)).toBe(25);
    expect(mediaPercent(200, 100)).toBe(100);
    expect(mediaPercent(-5, 100)).toBe(0);
  });

  it("keeps keyboard seeks inside the media duration", () => {
    expect(clampedSeekTime(4, -10, 100)).toBe(0);
    expect(clampedSeekTime(95, 10, 100)).toBe(100);
    expect(clampedSeekTime(25, 10, 100)).toBe(35);
  });

  it("aligns subtitle extraction with restarted playback", () => {
    expect(subtitleStreamUrl("http://127.0.0.1/subtitle/1/2/6?token=secret", 42.25)).toBe(
      "http://127.0.0.1/subtitle/1/2/6?token=secret&start=42.250",
    );
  });
});

describe("media bridge URLs", () => {
  it("preserves the capability token while selecting audio and a restart position", () => {
    expect(playbackStreamUrl("http://127.0.0.1/play/1/2?token=secret", 3, 42.25)).toBe(
      "http://127.0.0.1/play/1/2?token=secret&audio=3&start=42.250",
    );
  });
});

import { describe, expect, it } from "vitest";

import { formatBytes, formatDownloadSpeed, playbackPercent } from "./playback-model";

describe("playback progress formatting", () => {
  it("bounds transfer percentages", () => {
    expect(playbackPercent(25, 100)).toBe(25);
    expect(playbackPercent(200, 100)).toBe(100);
    expect(playbackPercent(1, 0)).toBe(0);
  });

  it("formats binary transfer sizes and speeds", () => {
    expect(formatBytes(1_048_576)).toBe("1.0 MiB");
    expect(formatBytes(2_058_991_637)).toBe("1.9 GiB");
    expect(formatDownloadSpeed(0.5)).toBe("512 KiB/s");
    expect(formatDownloadSpeed(2.25)).toBe("2.3 MiB/s");
  });
});

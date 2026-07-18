import type { PlaybackStage } from "../../shared/contract.generated";

export function playbackPercent(downloadedBytes: number, totalBytes: number) {
  if (!Number.isFinite(downloadedBytes) || !Number.isFinite(totalBytes) || totalBytes <= 0) {
    return 0;
  }
  return Math.min(100, Math.max(0, downloadedBytes / totalBytes * 100));
}

export function formatBytes(bytes: number) {
  if (!Number.isFinite(bytes) || bytes <= 0) return "0 B";
  const units = ["B", "KiB", "MiB", "GiB", "TiB"];
  const exponent = Math.min(Math.floor(Math.log(bytes) / Math.log(1024)), units.length - 1);
  const value = bytes / 1024 ** exponent;
  return `${value >= 100 || exponent === 0 ? value.toFixed(0) : value.toFixed(1)} ${units[exponent]}`;
}

export function formatDownloadSpeed(mebibytesPerSecond: number) {
  if (!Number.isFinite(mebibytesPerSecond) || mebibytesPerSecond <= 0) return "0 KiB/s";
  if (mebibytesPerSecond < 1) return `${Math.round(mebibytesPerSecond * 1024)} KiB/s`;
  return `${mebibytesPerSecond.toFixed(1)} MiB/s`;
}

export function playbackStageLabel(stage: PlaybackStage) {
  switch (stage) {
    case "resolving_metadata": return "Downloading torrent metadata…";
    case "validating_cache": return "Checking cached stream data…";
    case "buffering": return "Downloading stream data…";
    case "ready": return "Ready to play";
    case "failed": return "Couldn’t prepare playback";
  }
}

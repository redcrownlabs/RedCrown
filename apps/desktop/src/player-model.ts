export function formatPlaybackTime(seconds: number) {
  if (!Number.isFinite(seconds) || seconds < 0) return "0:00";
  const totalSeconds = Math.floor(seconds);
  const hours = Math.floor(totalSeconds / 3600);
  const minutes = Math.floor((totalSeconds % 3600) / 60);
  const remainingSeconds = totalSeconds % 60;
  return hours > 0
    ? `${hours}:${minutes.toString().padStart(2, "0")}:${remainingSeconds.toString().padStart(2, "0")}`
    : `${minutes}:${remainingSeconds.toString().padStart(2, "0")}`;
}

export function mediaPercent(value: number, total: number) {
  if (!Number.isFinite(value) || !Number.isFinite(total) || total <= 0) return 0;
  return Math.min(100, Math.max(0, (value / total) * 100));
}

export function clampedSeekTime(current: number, offset: number, duration: number) {
  if (!Number.isFinite(duration) || duration <= 0) return Math.max(0, current + offset);
  return Math.min(duration, Math.max(0, current + offset));
}

export function playbackStreamUrl(baseUrl: string, audioTrack: number | undefined, start: number) {
  const url = new URL(baseUrl);
  if (audioTrack == null) url.searchParams.delete("audio");
  else url.searchParams.set("audio", String(audioTrack));
  if (Number.isFinite(start) && start > 0) url.searchParams.set("start", start.toFixed(3));
  else url.searchParams.delete("start");
  return url.toString();
}

export function subtitleStreamUrl(baseUrl: string, start: number) {
  const url = new URL(baseUrl);
  if (Number.isFinite(start) && start > 0) url.searchParams.set("start", start.toFixed(3));
  else url.searchParams.delete("start");
  return url.toString();
}

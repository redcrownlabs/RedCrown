import { useEffect, useState } from "react";
import type { MediaItem, PlaybackStatus } from "../../shared/contract.generated";
import { formatBytes, formatDownloadSpeed, playbackPercent, playbackStageLabel } from "./playback-model";
import { invoke, messageOf } from "../../shared/ipc";
import { Icon } from "../../shared/ui/Icon";
import { PosterImage } from "../../shared/ui/PosterImage";
import { RedCrownPlayer } from "./RedCrownPlayer";

export function PlayerView({
  item,
  initialStatus,
  onClose,
}: {
  item: MediaItem;
  initialStatus: PlaybackStatus;
  onClose: () => void;
}) {
  const [status, setStatus] = useState(initialStatus);

  useEffect(() => {
    let active = true;
    let timer: ReturnType<typeof setTimeout> | undefined;
    async function poll() {
      try {
        const next = await invoke<PlaybackStatus>("playback.status", {
          preparation_id: initialStatus.preparation_id,
        });
        if (!active) return;
        setStatus(next);
        if (next.stage !== "failed") {
          timer = setTimeout(() => void poll(), 750);
        }
      } catch (reason) {
        if (!active) return;
        setStatus((current) => ({
          ...current,
          stage: "failed",
          error: messageOf(reason),
        }));
      }
    }
    void poll();
    return () => {
      active = false;
      if (timer) clearTimeout(timer);
    };
  }, [initialStatus.preparation_id]);

  const percent = playbackPercent(status.downloaded_bytes, status.total_bytes);
  const ready = status.stage === "ready" && status.ticket;
  const hasProgress = status.total_bytes > 0;
  const preparing = status.stage !== "failed";

  if (!ready) {
    return (
      <section
        className="playback-scene"
        style={item.backdrop_url ? { backgroundImage: `url("${item.backdrop_url}")` } : undefined}
      >
        <div className="playback-shade">
          <button className="playback-dismiss" onClick={onClose} type="button">
            <Icon name="back" />
            Cancel playback
          </button>
          <div className="playback-focus">
            {item.poster_url ? (
              <div className="playback-poster" aria-hidden="true">
                <PosterImage src={item.poster_url} fallback={item.title[0]} loading="eager" />
              </div>
            ) : (
              <div className="playback-poster playback-poster-fallback" aria-hidden="true">
                {item.title[0]}
              </div>
            )}

            <div className="playback-copy">
              <h1>{item.title}</h1>
              <p className="playback-stage">
                {status.stage === "resolving_metadata"
                  ? "Downloading torrent metadata…"
                  : status.stage === "failed"
                    ? playbackStageLabel(status.stage)
                    : "\u00a0"}
              </p>

              {preparing && (
                <div
                  className={`playback-meter${hasProgress ? "" : " is-waiting"}`}
                  role="progressbar"
                  aria-label="Stream preparation progress"
                  aria-valuemin={0}
                  aria-valuemax={100}
                  aria-valuenow={hasProgress ? Math.round(percent) : undefined}
                >
                  <span style={hasProgress ? { width: `${percent}%` } : undefined} />
                </div>
              )}

              {hasProgress && (
                <p className="playback-percent">{percent.toFixed(0)}%</p>
              )}

              <PlaybackTransfer status={status} />
              {status.error && <p className="playback-error" role="alert">{status.error}</p>}
            </div>
          </div>
        </div>
      </section>
    );
  }

  return <RedCrownPlayer item={item} status={status} onClose={onClose} />;
}


function PlaybackTransfer({ status }: { status: PlaybackStatus }) {
  const metadataPending = status.stage === "resolving_metadata";

  return (
    <dl className={`playback-transfer${metadataPending ? " is-pending" : ""}`} aria-hidden={metadataPending}>
      <div>
        <dt>Speed</dt>
        <dd>{formatDownloadSpeed(status.download_mib_per_second)}</dd>
      </div>
      <div>
        <dt>Peers</dt>
        <dd>{status.connected_peers}</dd>
      </div>
      <div>
        <dt>Downloaded</dt>
        <dd>
          {formatBytes(status.downloaded_bytes)}
          {status.total_bytes > 0 ? ` / ${formatBytes(status.total_bytes)}` : ""}
        </dd>
      </div>
    </dl>
  );
}

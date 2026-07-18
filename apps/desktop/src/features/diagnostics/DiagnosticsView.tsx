import { useEffect, useState } from "react";
import type { ReactNode } from "react";
import type { TorrentDiagnostics } from "../../shared/contract.generated";
import { formatBytes, formatDownloadSpeed, playbackPercent, playbackStageLabel } from "../playback/playback-model";
import { invoke, messageOf } from "../../shared/ipc";
import { Icon } from "../../shared/ui/Icon";
import { startDiagnosticsPolling } from "./diagnostics-model";

export function DiagnosticsView({
  preparationId,
  onBack,
}: {
  preparationId?: string;
  onBack: () => void;
}) {
  const [diagnostics, setDiagnostics] = useState<TorrentDiagnostics>();
  const [diagnosticError, setDiagnosticError] = useState<string>();
  const [magnetCopied, setMagnetCopied] = useState(false);

  useEffect(() => {
    if (!preparationId) return;
    return startDiagnosticsPolling(
      () => invoke<TorrentDiagnostics>("playback.diagnostics", {
          preparation_id: preparationId,
      }),
      setDiagnostics,
      setDiagnosticError,
      messageOf,
    );
  }, [preparationId]);

  async function copyMagnetLink() {
    const magnetLink = diagnostics?.magnet_link;
    if (!magnetLink) return;
    try {
      await navigator.clipboard.writeText(magnetLink);
      setMagnetCopied(true);
    } catch (reason) {
      setDiagnosticError(`Could not copy magnet link: ${messageOf(reason)}`);
    }
  }

  if (!preparationId) {
    return (
      <section className="diagnostics-view">
        <header className="diagnostics-header">
          <div><p className="eyebrow">Torrent engine</p><h1>Diagnostics</h1></div>
          <button className="secondary-button" onClick={onBack}><Icon name="back" />Back</button>
        </header>
        <div className="diagnostics-empty">
          <h2>No active playback</h2>
          <p>Start a title, then open this screen to inspect its transfer.</p>
        </div>
      </section>
    );
  }

  const playback = diagnostics?.playback;
  const percent = playback
    ? playbackPercent(playback.downloaded_bytes, playback.total_bytes)
    : 0;
  const piecePercent = diagnostics && diagnostics.pieces.total > 0
    ? Math.min(100, diagnostics.pieces.available / diagnostics.pieces.total * 100)
    : 0;

  return (
    <section className="diagnostics-view">
      <header className="diagnostics-header">
        <div>
          <p className="eyebrow">Torrent engine</p>
          <h1>Diagnostics</h1>
          <p>Live internals for the current stream.</p>
        </div>
        <button className="secondary-button" onClick={onBack}><Icon name="back" />Return</button>
      </header>

      {diagnosticError && <p className="diagnostics-error" role="alert">{diagnosticError}</p>}
      {!diagnostics ? (
        <div className="diagnostics-empty"><p>Reading torrent state…</p></div>
      ) : (
        <div className="diagnostics-grid">
          <section className="diagnostics-card diagnostics-overview">
            <div className="diagnostics-card-heading">
              <div><p className="diagnostics-label">State</p><h2>Transfer</h2></div>
              <span className="diagnostics-state">{diagnostics.engine_state ?? playbackStageLabel(diagnostics.playback.stage)}</span>
            </div>
            <div className="diagnostics-progress" aria-hidden="true"><span style={{ width: `${percent}%` }} /></div>
            <strong className="diagnostics-progress-copy">{percent.toFixed(1)}%</strong>
            <dl className="diagnostics-stats">
              <DiagnosticStat label="Available locally" value={`${formatBytes(diagnostics.playback.downloaded_bytes)} / ${formatBytes(diagnostics.playback.total_bytes)}`} />
              <DiagnosticStat label="Downloaded this session" value={formatBytes(diagnostics.downloaded_this_session_bytes)} />
              <DiagnosticStat label="Download now" value={formatDownloadSpeed(diagnostics.playback.download_mib_per_second)} />
              <DiagnosticStat label="Uploaded this session" value={formatBytes(diagnostics.uploaded_bytes)} />
              <DiagnosticStat label="Upload now" value={formatDownloadSpeed(diagnostics.upload_mib_per_second)} />
            </dl>
            {diagnostics.playback.error && <p className="diagnostics-error" role="alert">{diagnostics.playback.error}</p>}
          </section>

          <section className="diagnostics-card">
            <div className="diagnostics-card-heading"><h2>Peers</h2><span>{diagnostics.peers.connected} connected</span></div>
            <dl className="diagnostics-stats diagnostics-stats-compact">
              <DiagnosticStat label="Seen" value={diagnostics.peers.seen} />
              <DiagnosticStat label="Queued" value={diagnostics.peers.queued} />
              <DiagnosticStat label="Connecting" value={diagnostics.peers.connecting} />
              <DiagnosticStat label="Dead" value={diagnostics.peers.dead} />
              <DiagnosticStat label="Not needed" value={diagnostics.peers.not_needed} />
              <DiagnosticStat label="Seeders" value={diagnostics.peers.seeders ?? "Not exposed"} />
            </dl>
          </section>

          <section className="diagnostics-card">
            <div className="diagnostics-card-heading"><h2>Pieces</h2><span>{piecePercent.toFixed(1)}%</span></div>
            <div className="diagnostics-progress piece-progress" aria-hidden="true"><span style={{ width: `${piecePercent}%` }} /></div>
            <dl className="diagnostics-stats diagnostics-stats-compact">
              <DiagnosticStat label="Available" value={diagnostics.pieces.available} />
              <DiagnosticStat label="Downloaded this session" value={diagnostics.pieces.downloaded_this_session} />
              <DiagnosticStat label="Total" value={diagnostics.pieces.total || "Resolving"} />
              <DiagnosticStat label="Average piece" value={diagnostics.pieces.average_download_ms != null ? `${diagnostics.pieces.average_download_ms} ms` : "Waiting"} />
            </dl>
          </section>

          <section className="diagnostics-card">
            <div className="diagnostics-card-heading"><h2>Discovery</h2><span>DHT</span></div>
            <dl className="diagnostics-stats diagnostics-stats-compact">
              <DiagnosticStat label="Routing nodes" value={diagnostics.dht?.routing_table_size ?? "Unavailable"} />
              <DiagnosticStat label="Open requests" value={diagnostics.dht?.outstanding_requests ?? "Unavailable"} />
            </dl>
            {diagnostics.dht && <code className="diagnostics-hash">Node {diagnostics.dht.node_id}</code>}
          </section>

          <section className="diagnostics-card diagnostics-trackers">
            <div className="diagnostics-card-heading"><h2>Trackers</h2><span>{diagnostics.trackers.length} configured</span></div>
            {diagnostics.trackers.length ? (
              <ul>{diagnostics.trackers.map((tracker) => <li key={tracker}><code>{tracker}</code></li>)}</ul>
            ) : <p>No tracker URLs; peer discovery depends on DHT or initial peers.</p>}
          </section>

          <section className="diagnostics-card diagnostics-identity">
            <div className="diagnostics-card-heading"><h2>Identity</h2></div>
            <dl>
              <div><dt>Info hash</dt><dd><code>{diagnostics.info_hash ?? "Resolving metadata"}</code></dd></div>
              <div className="diagnostics-magnet">
                <dt>Magnet link</dt>
                <dd>
                  {diagnostics.magnet_link ? (
                    <div className="magnet-value">
                      <code>{diagnostics.magnet_link}</code>
                      <button type="button" onClick={() => void copyMagnetLink()}>
                        {magnetCopied ? "Copied" : "Copy magnet"}
                      </button>
                    </div>
                  ) : <span className="diagnostics-unavailable">Not a magnet source</span>}
                </dd>
              </div>
              <div><dt>Media file</dt><dd>{diagnostics.playback.ticket?.file_name ?? "Resolving metadata"}</dd></div>
            </dl>
          </section>
        </div>
      )}
    </section>
  );
}

function DiagnosticStat({ label, value }: { label: string; value: ReactNode }) {
  return <div><dt>{label}</dt><dd>{value}</dd></div>;
}

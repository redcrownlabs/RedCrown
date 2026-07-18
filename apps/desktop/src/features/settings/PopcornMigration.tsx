import { useEffect, useState } from "react";
import type { PopcornImportReport, PopcornImportSelection, PopcornProfilePreview } from "../../shared/contract.generated";
import { invoke, messageOf } from "../../shared/ipc";

export function PopcornMigration({
  onImported,
}: {
  onImported: (report: PopcornImportReport) => void;
}) {
  const [profiles, setProfiles] = useState<PopcornProfilePreview[]>([]);
  const [selectedId, setSelectedId] = useState<string>();
  const [selection, setSelection] = useState<PopcornImportSelection>({
    api_urls: true,
    favorites: true,
    watched: true,
    playback_progress: true,
  });
  const [status, setStatus] = useState("Looking for an existing Popcorn Time profile…");
  const [busy, setBusy] = useState(false);
  const selected = profiles.find((profile) => profile.id === selectedId);

  async function scan() {
    setBusy(true);
    setStatus("Looking for an existing Popcorn Time profile…");
    try {
      const found = await invoke<PopcornProfilePreview[]>("migration.popcorn.discover");
      setProfiles(found);
      setSelectedId((current) =>
        current && found.some((profile) => profile.id === current) ? current : found[0]?.id,
      );
      setStatus(found.length ? `${found.length} compatible profile${found.length === 1 ? "" : "s"} found` : "No compatible profile found");
    } catch (reason) {
      setStatus(messageOf(reason));
    } finally {
      setBusy(false);
    }
  }

  useEffect(() => {
    void scan();
  }, []);

  async function importProfile() {
    if (!selected) return;
    setBusy(true);
    setStatus("Importing selected data…");
    try {
      const report = await invoke<PopcornImportReport>("migration.popcorn.import", {
        profile_id: selected.id,
        selection,
      });
      onImported(report);
      const imported = [
        `${report.api_urls_added} API URLs`,
        `${report.library.favorites_imported} favorites`,
        `${report.library.watched_movies_imported} watched movies`,
        `${report.library.watched_episodes_imported} watched episodes`,
      ];
      setStatus(`Import complete: ${imported.join(", ")}.`);
    } catch (reason) {
      setStatus(messageOf(reason));
    } finally {
      setBusy(false);
    }
  }

  return (
    <section className="settings-section migration-section" aria-labelledby="migration-heading">
      <div className="settings-intro">
        <div>
          <p className="section-kicker">One-time migration</p>
          <h2 id="migration-heading">Import from Popcorn Time</h2>
          <p>Preview and copy compatible data. The original profile stays unchanged.</p>
        </div>
        <button className="secondary-button" disabled={busy} onClick={() => void scan()}>Scan again</button>
      </div>
      {selected ? (
        <div className="migration-preview">
          {profiles.length > 1 && (
            <label className="field">
              <span>Profile</span>
              <select className="branded-select" value={selected.id} onChange={(event) => setSelectedId(event.target.value)}>
                {profiles.map((profile) => (
                  <option value={profile.id} key={profile.id}>
                    {profile.label} {profile.version ? ` ${profile.version}` : ""}
                  </option>
                ))}
              </select>
            </label>
          )}
          <div className="profile-heading">
            <div>
              <strong>{selected.label}{selected.version ? ` ${selected.version}` : ""}</strong>
              <span>{selected.modified_at_ms ? `Updated ${new Date(selected.modified_at_ms).toLocaleDateString()}` : "Modification date unavailable"}</span>
            </div>
            <span className="safe-badge">Read-only source</span>
          </div>
          <div className="import-options">
            <ImportOption
              checked={selection.api_urls}
              disabled={!selected.api_urls.length}
              title="Catalog API URLs"
              detail={`${selected.api_urls.length} ordered fallback${selected.api_urls.length === 1 ? "" : "s"}`}
              onChange={(api_urls) => setSelection((current) => ({ ...current, api_urls }))}
            />
            <ImportOption
              checked={selection.favorites}
              disabled={!selected.favorite_count}
              title="Favorites"
              detail={`${selected.favorite_count} saved title${selected.favorite_count === 1 ? "" : "s"}`}
              onChange={(favorites) => setSelection((current) => ({ ...current, favorites }))}
            />
            <ImportOption
              checked={selection.watched}
              disabled={!selected.watched_movie_count && !selected.watched_episode_count}
              title="Watched history"
              detail={`${selected.watched_movie_count} movies · ${selected.watched_episode_count} episodes`}
              onChange={(watched) => setSelection((current) => ({ ...current, watched }))}
            />
            <ImportOption
              checked={selection.playback_progress}
              disabled={!selected.has_playback_progress}
              title="Playback position"
              detail={selected.has_playback_progress ? "One safely matched item" : "No unambiguous position found"}
              onChange={(playback_progress) => setSelection((current) => ({ ...current, playback_progress }))}
            />
          </div>
          {selected.api_urls.length > 0 && selection.api_urls && (
            <details className="endpoint-preview">
              <summary>Review API URLs</summary>
              <ol>{selected.api_urls.map((url) => <li key={url}>{url}</li>)}</ol>
            </details>
          )}
          {selected.notes.map((note) => <p className="migration-note" key={note}>{note}</p>)}
          <div className="settings-actions migration-actions">
            <span role="status" aria-live="polite">{status}</span>
            <button
              className="primary-button"
              disabled={busy || !Object.values(selection).some(Boolean)}
              onClick={() => void importProfile()}
            >
              {busy ? "Working…" : "Import selected"}
            </button>
          </div>
        </div>
      ) : (
        <div className="migration-empty">
          <p>{status}</p>
          <span>Supported desktop profiles are detected from their standard Windows data location.</span>
        </div>
      )}
    </section>
  );
}

function ImportOption({
  checked,
  disabled,
  title,
  detail,
  onChange,
}: {
  checked: boolean;
  disabled: boolean;
  title: string;
  detail: string;
  onChange: (checked: boolean) => void;
}) {
  return (
    <label className={`import-option${disabled ? " disabled" : ""}`}>
      <input
        type="checkbox"
        checked={checked && !disabled}
        disabled={disabled}
        onChange={(event) => onChange(event.target.checked)}
      />
      <span><strong>{title}</strong><small>{detail}</small></span>
    </label>
  );
}

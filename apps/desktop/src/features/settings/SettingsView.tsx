import { useState } from "react";
import type { AppSettings, EndpointHealth, LibrarySummary, SourceConfig } from "../../shared/contract.generated";
import { invoke, messageOf } from "../../shared/ipc";
import { PopcornMigration } from "./PopcornMigration";
import {
  moveEndpoint,
  normalizeEndpoint,
  validateSource,
  validateTrackerList,
} from "./settings-model";

const DEFAULT_TRACKER_LIST_URL =
  "https://raw.githubusercontent.com/ngosang/trackerslist/refs/heads/master/trackers_all.txt";

export function SettingsView({
  initial,
  configurationRequired,
  onSaved,
  onLibraryImported,
}: {
  initial: AppSettings;
  configurationRequired: boolean;
  onSaved: (settings: AppSettings) => void;
  onLibraryImported: (library: LibrarySummary) => void;
}) {
  const [draft, setDraft] = useState(() => structuredClone(initial));
  const [health, setHealth] = useState<Record<string, EndpointHealth>>({});
  const [status, setStatus] = useState<string>();
  const [trackerTouched, setTrackerTouched] = useState(false);
  const source = draft.sources[0];
  const sourceValidation = source ? validateSource(source) : "A source is required";
  const trackerValidation = validateTrackerList(draft.tracker_list);
  const validation = sourceValidation ?? trackerValidation;

  function updateSource(update: (source: SourceConfig) => SourceConfig) {
    setDraft((current) => ({
      ...current,
      sources: current.sources.map((entry, index) => index === 0 ? update(entry) : entry),
    }));
  }

  async function testSource() {
    if (!source || sourceValidation) return;
    setStatus("Testing fallback chain…");
    try {
      const result = await invoke<EndpointHealth[]>("source.test", { source });
      setHealth(Object.fromEntries(result.map((entry) => [entry.endpoint_id, entry])));
      setStatus("Test complete");
    } catch (reason) {
      setStatus(messageOf(reason));
    }
  }

  async function save() {
    setTrackerTouched(true);
    if (!source || validation) return;
    setStatus("Saving and importing trackers…");
    try {
      const normalized = {
        ...draft,
        sources: draft.sources.map((entry) => ({
          ...entry,
          endpoints: entry.endpoints.map((endpoint) => ({
            ...endpoint,
            url: normalizeEndpoint(endpoint.url),
          })),
        })),
        tracker_list: {
          ...draft.tracker_list,
          source: draft.tracker_list.source.kind === "url"
            ? { kind: "url" as const, url: new URL(draft.tracker_list.source.url.trim()).toString() }
            : { kind: "file" as const, path: draft.tracker_list.source.path.trim() },
        },
      };
      const saved = await invoke<AppSettings>("settings.save", { settings: normalized });
      setDraft(saved);
      onSaved(saved);
      setStatus("Saved");
    } catch (reason) {
      setStatus(messageOf(reason));
    }
  }

  return (
    <div className="settings-view">
      <header className="page-header">
        <div><p className="eyebrow">RedCrown</p><h1>Settings</h1><p>Sources, migration, and temporary storage.</p></div>
        <div className="settings-header-actions">
          <span aria-live="polite">{status}</span>
          <button className="primary-button" disabled={Boolean(validation)} onClick={() => void save()}>Save settings</button>
        </div>
      </header>
      {configurationRequired && (
        <section className="setup-notice" aria-labelledby="setup-title">
          <p className="section-kicker">First-run setup</p>
          <h2 id="setup-title">Connect a catalog source</h2>
          <p>
            RedCrown does not bundle a catalog service. Add at least one compatible API URL,
            test the fallback chain, and save it to begin browsing.
          </p>
        </section>
      )}
      <PopcornMigration
        onImported={(report) => {
          setDraft(report.settings);
          onSaved(report.settings);
          onLibraryImported(report.library.library);
        }}
      />
      <section className="settings-section">
        <div className="settings-intro">
          <div><h2>Catalog API URLs</h2><p>Ordered fallbacks for one compatible source. RedCrown tries them in this order.</p></div>
          <label className="switch"><input type="checkbox" checked={source?.enabled ?? false} onChange={(event) => updateSource((entry) => ({ ...entry, enabled: event.target.checked }))} /><span>Source enabled</span></label>
        </div>
        <label className="field">
          <span>Source name</span>
          <input value={source?.name ?? ""} onChange={(event) => updateSource((entry) => ({ ...entry, name: event.target.value }))} />
        </label>
        <div className="endpoint-list">
          {source?.endpoints.map((endpoint, index) => (
            <div className="endpoint-row" key={endpoint.id}>
              <span className="order-number">{index + 1}</span>
              <label className="field endpoint-field">
                <span>Fallback URL {index + 1}</span>
                <input value={endpoint.url} onChange={(event) => updateSource((entry) => ({ ...entry, endpoints: entry.endpoints.map((item) => item.id === endpoint.id ? { ...item, url: event.target.value } : item) }))} />
              </label>
              <label className="icon-toggle" title="Enable URL"><input type="checkbox" checked={endpoint.enabled} onChange={(event) => updateSource((entry) => ({ ...entry, endpoints: entry.endpoints.map((item) => item.id === endpoint.id ? { ...item, enabled: event.target.checked } : item) }))} /><span>On</span></label>
              <div className="row-actions">
                <button aria-label="Move URL up" disabled={index === 0} onClick={() => updateSource((entry) => ({ ...entry, endpoints: moveEndpoint(entry.endpoints, index, -1) }))}>↑</button>
                <button aria-label="Move URL down" disabled={index === source.endpoints.length - 1} onClick={() => updateSource((entry) => ({ ...entry, endpoints: moveEndpoint(entry.endpoints, index, 1) }))}>↓</button>
                <button aria-label="Remove URL" onClick={() => updateSource((entry) => ({ ...entry, endpoints: entry.endpoints.filter((item) => item.id !== endpoint.id) }))}>×</button>
              </div>
              {health[endpoint.id] && <span className={health[endpoint.id].reachable ? "health good" : "health bad"}>{health[endpoint.id].reachable ? `${health[endpoint.id].latency_ms} ms` : health[endpoint.id].message}</span>}
            </div>
          ))}
        </div>
        <button className="secondary-button" onClick={() => updateSource((entry) => ({ ...entry, endpoints: [...entry.endpoints, { id: crypto.randomUUID(), url: "https://", enabled: true }] }))}>Add fallback URL</button>
        {sourceValidation && <p className="field-error">{sourceValidation}</p>}
        <div className="settings-actions endpoint-test-actions">
          <button className="secondary-button" disabled={Boolean(sourceValidation)} onClick={() => void testSource()}>Test all</button>
        </div>
      </section>
      <section className="settings-section" aria-labelledby="tracker-list-heading">
        <div className="settings-intro">
          <div>
            <h2 id="tracker-list-heading">Supplemental tracker list</h2>
            <p>Used only when a magnet contains no trackers. The list refreshes daily and keeps a last-known-good copy.</p>
          </div>
          <label className="switch">
            <input
              type="checkbox"
              checked={draft.tracker_list.enabled}
              onChange={(event) => setDraft((current) => ({
                ...current,
                tracker_list: { ...current.tracker_list, enabled: event.target.checked },
              }))}
            />
            <span>Import enabled</span>
          </label>
        </div>
        <div className="tracker-source-kind" role="group" aria-label="Tracker-list source type">
          <button
            type="button"
            aria-pressed={draft.tracker_list.source.kind === "url"}
            className={draft.tracker_list.source.kind === "url" ? "is-active" : undefined}
            onClick={() => setDraft((current) => ({
              ...current,
              tracker_list: {
                ...current.tracker_list,
                source: { kind: "url", url: DEFAULT_TRACKER_LIST_URL },
              },
            }))}
          >
            HTTPS URL
          </button>
          <button
            type="button"
            aria-pressed={draft.tracker_list.source.kind === "file"}
            className={draft.tracker_list.source.kind === "file" ? "is-active" : undefined}
            onClick={() => setDraft((current) => ({
              ...current,
              tracker_list: {
                ...current.tracker_list,
                source: { kind: "file", path: "" },
              },
            }))}
          >
            Local file
          </button>
        </div>
        {draft.tracker_list.source.kind === "url" ? (
          <label className="field tracker-source-field">
            <span>Tracker-list URL</span>
            <input
              type="url"
              name="tracker-list-url"
              value={draft.tracker_list.source.url}
              aria-describedby="tracker-list-help tracker-list-error"
              aria-invalid={trackerTouched && Boolean(trackerValidation)}
              onBlur={() => setTrackerTouched(true)}
              onChange={(event) => setDraft((current) => ({
                ...current,
                tracker_list: {
                  ...current.tracker_list,
                  source: { kind: "url", url: event.target.value },
                },
              }))}
            />
          </label>
        ) : (
          <label className="field tracker-source-field">
            <span>Absolute tracker-list path</span>
            <input
              name="tracker-list-path"
              value={draft.tracker_list.source.path}
              placeholder="C:\\trackers\\trackers.txt"
              aria-describedby="tracker-list-help tracker-list-error"
              aria-invalid={trackerTouched && Boolean(trackerValidation)}
              onBlur={() => setTrackerTouched(true)}
              onChange={(event) => setDraft((current) => ({
                ...current,
                tracker_list: {
                  ...current.tracker_list,
                  source: { kind: "file", path: event.target.value },
                },
              }))}
            />
          </label>
        )}
        <p id="tracker-list-help" className="field-help">Whitespace-separated HTTP, HTTPS, and UDP tracker URLs; maximum 1 MiB and 512 trackers.</p>
        <p id="tracker-list-error" className="field-error" aria-live="polite">
          {trackerTouched ? trackerValidation : undefined}
        </p>
      </section>
      <section className="settings-section">
        <div className="settings-intro"><div><h2>Temporary stream cache</h2><p>Reusable only for a short period. Active playback is never evicted.</p></div></div>
        <div className="storage-grid">
          <label className="field"><span>Idle expiration (hours)</span><input type="number" min="1" max="168" value={draft.stream_cache.idle_expiration_secs / 3600} onChange={(event) => setDraft((current) => ({ ...current, stream_cache: { ...current.stream_cache, idle_expiration_secs: Number(event.target.value) * 3600 } }))} /></label>
          <label className="field"><span>Maximum age (hours)</span><input type="number" min="1" max="168" value={draft.stream_cache.maximum_age_secs / 3600} onChange={(event) => setDraft((current) => ({ ...current, stream_cache: { ...current.stream_cache, maximum_age_secs: Number(event.target.value) * 3600 } }))} /></label>
          <label className="field"><span>Size budget (GiB)</span><input type="number" min="1" max="500" value={Math.round(draft.stream_cache.size_budget_bytes / 1073741824)} onChange={(event) => setDraft((current) => ({ ...current, stream_cache: { ...current.stream_cache, size_budget_bytes: Number(event.target.value) * 1073741824 } }))} /></label>
        </div>
      </section>
    </div>
  );
}

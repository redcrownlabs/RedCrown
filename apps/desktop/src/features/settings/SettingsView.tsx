import { useState } from "react";
import type { AppSettings, EndpointHealth, LibrarySummary, SourceConfig } from "../../shared/contract.generated";
import { invoke, messageOf } from "../../shared/ipc";
import { PopcornMigration } from "./PopcornMigration";
import { moveEndpoint, normalizeEndpoint, validateSource } from "./settings-model";

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
  const source = draft.sources[0];
  const validation = source ? validateSource(source) : "A source is required";

  function updateSource(update: (source: SourceConfig) => SourceConfig) {
    setDraft((current) => ({
      ...current,
      sources: current.sources.map((entry, index) => index === 0 ? update(entry) : entry),
    }));
  }

  async function testSource() {
    if (!source || validation) return;
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
    if (!source || validation) return;
    setStatus("Saving…");
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
        {validation && <p className="field-error">{validation}</p>}
        <div className="settings-actions">
          <span aria-live="polite">{status}</span>
          <button className="secondary-button" disabled={Boolean(validation)} onClick={() => void testSource()}>Test all</button>
          <button className="primary-button" disabled={Boolean(validation)} onClick={() => void save()}>Save</button>
        </div>
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


import { startTransition, useCallback, useEffect, useState } from "react";

import type { BootstrapState, CatalogPage, CatalogQuery, LibrarySummary, MediaItem, MediaKind, PlaybackStatus, TorrentOption } from "../shared/contract.generated";
import { visibleHomeItems } from "../features/home/home-model";
import { hasConfiguredCatalog } from "../features/settings/settings-model";
import { CatalogView } from "../features/catalog/CatalogView";
import { catalogQuery } from "../features/catalog/catalog-utils";
import { DetailsView } from "../features/details/DetailsView";
import { DiagnosticsView } from "../features/diagnostics/DiagnosticsView";
import { HomeView } from "../features/home/HomeView";
import type { HomeSection } from "../features/home/HomeView";
import { LibraryView } from "../features/library/LibraryView";
import { PlayerView } from "../features/playback/PlayerView";
import { SettingsView } from "../features/settings/SettingsView";
import { invoke, messageOf } from "../shared/ipc";
import { Icon } from "../shared/ui/Icon";
import { WindowControls } from "../shared/ui/WindowControls";

type View = "home" | "catalog" | "library" | "settings" | "details" | "player" | "diagnostics";

const HOME_CATALOG_REQUESTS: ReadonlyArray<{ title: string; query: CatalogQuery }> = [
  { title: "Trending movies", query: catalogQuery("movie", "trending") },
  { title: "Popular series", query: catalogQuery("series", "popularity") },
  { title: "Anime right now", query: catalogQuery("anime", "trending") },
  { title: "Recently added movies", query: catalogQuery("movie", "last_added") },
  { title: "Top rated", query: catalogQuery("series", "rating") },
];

export function App() {
  const [bootstrap, setBootstrap] = useState<BootstrapState>();
  const [homeSections, setHomeSections] = useState<HomeSection[]>([]);
  const [view, setView] = useState<View>("home");
  const [catalogKind, setCatalogKind] = useState<MediaKind>("movie");
  const [detailsReturnView, setDetailsReturnView] = useState<View>("home");
  const [selected, setSelected] = useState<MediaItem>();
  const [playback, setPlayback] = useState<PlaybackStatus>();
  const [library, setLibrary] = useState<LibrarySummary>();
  const [error, setError] = useState<string>();
  const [busy, setBusy] = useState(true);
  const watchedMovies = library?.watched_movies ?? [];
  const visibleHomeSections = homeSections
    .map((section) => ({
      ...section,
      items: visibleHomeItems(section.items, watchedMovies),
    }))
    .filter((section) => section.items.length > 0);
  const visibleFeatured = visibleHomeItems(bootstrap?.featured ?? [], watchedMovies);

  const refreshHome = useCallback(async () => {
    const results = await Promise.allSettled(
      HOME_CATALOG_REQUESTS.map(({ query }) =>
        invoke<CatalogPage>("catalog.browse", { ...query }),
      ),
    );
    const sections = results.flatMap((result, index) =>
      result.status === "fulfilled" && result.value.items.length
        ? [{
            title: HOME_CATALOG_REQUESTS[index].title,
            kind: HOME_CATALOG_REQUESTS[index].query.kind,
            items: result.value.items,
          }]
        : [],
    );
    setHomeSections(sections);
    if (!sections.length) {
      setError("The configured catalog APIs did not return any discovery rows.");
      return false;
    }
    setError(undefined);
    return true;
  }, []);

  useEffect(() => {
    let active = true;
    void Promise.all([invoke<BootstrapState>("bootstrap"), invoke<LibrarySummary>("library.summary")])
      .then(async ([state, libraryState]) => {
        if (!active) return;
        setBootstrap(state);
        setLibrary(libraryState);
        if (!hasConfiguredCatalog(state.settings)) {
          setView("settings");
          setError(undefined);
          return;
        }
        await refreshHome();
      })
      .catch((reason: unknown) => {
        if (active) setError(messageOf(reason));
      })
      .finally(() => {
        if (active) setBusy(false);
      });
    return () => {
      active = false;
    };
  }, [refreshHome]);

  const navigate = (next: View) => startTransition(() => setView(next));

  useEffect(() => {
    function toggleDiagnostics(event: KeyboardEvent) {
      if (event.repeat || !event.ctrlKey || !event.shiftKey || event.key.toLowerCase() !== "d") {
        return;
      }
      event.preventDefault();
      startTransition(() => setView((current) =>
        current === "diagnostics" ? (playback ? "player" : "home") : "diagnostics",
      ));
    }
    window.addEventListener("keydown", toggleDiagnostics);
    return () => window.removeEventListener("keydown", toggleDiagnostics);
  }, [playback]);

  function openCatalog(kind: MediaKind) {
    setCatalogKind(kind);
    navigate("catalog");
  }

  function openDetails(item: MediaItem, from: View) {
    setSelected(item);
    setDetailsReturnView(from);
    navigate("details");
  }

  async function watch(source: TorrentOption) {
    setBusy(true);
    setError(undefined);
    try {
      const preparation = await invoke<PlaybackStatus>("playback.prepare", {
        source: source.source,
        file_path: source.file_path,
      });
      setPlayback(preparation);
      navigate("player");
    } catch (reason) {
      setError(messageOf(reason));
    } finally {
      setBusy(false);
    }
  }

  async function closePlayer() {
    const activePlayback = playback;
    setPlayback(undefined);
    navigate("details");
    if (activePlayback) {
      await invoke("playback.cancel", {
        preparation_id: activePlayback.preparation_id,
      }).catch(() => undefined);
    }
  }

  if (!bootstrap) {
    return (
      <div className="startup-shell">
        <header className="startup-titlebar">
          <div className="startup-brand" aria-hidden="true" />
          <WindowControls />
        </header>
        <main className="center-state">
          <img className="brand-mark" src="/redcrown-mark.svg" alt="RedCrown" />
          <p>{error ?? "Starting RedCrown…"}</p>
        </main>
      </div>
    );
  }

  return (
    <div className={`app-shell${view === "player" ? " player-mode" : ""}`} data-theme={bootstrap.settings.theme}>
      <a className="skip-link" href="#main-content">Skip to content</a>
      <header className="top-bar">
        <button className="wordmark" onClick={() => navigate("home")} aria-label="RedCrown home">
          <img src="/redcrown-mark.svg" alt="" />RedCrown
        </button>
        <nav aria-label="Primary">
          <button className={view === "home" ? "active" : ""} onClick={() => navigate("home")}>Home</button>
          <button className={view === "catalog" ? "active" : ""} onClick={() => openCatalog("movie")}>Movies</button>
          <button className={view === "catalog" ? "active" : ""} onClick={() => openCatalog("series")}>Series</button>
          <button className={view === "catalog" ? "active" : ""} onClick={() => openCatalog("anime")}>Anime</button>
          <button className={view === "library" ? "active" : ""} onClick={() => navigate("library")}>My library</button>
        </nav>
        <div className="top-actions">
          <button className="icon-button" onClick={() => navigate("catalog")} aria-label="Search catalog"><Icon name="search" /></button>
          <button
            className={`icon-button${view === "diagnostics" ? " active" : ""}`}
            onClick={() => navigate("diagnostics")}
            aria-label="Open torrent diagnostics"
            aria-keyshortcuts="Control+Shift+D"
          >
            <Icon name="activity" />
          </button>
          <button className="icon-button" onClick={() => navigate("settings")} aria-label="Open settings"><Icon name="settings" /></button>
        </div>
        <WindowControls />
      </header>

      <main className="main-surface" id="main-content" tabIndex={-1}>
        {error && (
          <div className="error-banner" role="alert">
            <span>{error}</span>
            <button onClick={() => setError(undefined)}>Dismiss</button>
          </div>
        )}
        {view === "home" && (
          <HomeView
            sections={visibleHomeSections}
            fallback={visibleFeatured}
            busy={busy}
            onOpen={(item) => openDetails(item, "home")}
            onBrowse={openCatalog}
          />
        )}
        {view === "catalog" && (
          <CatalogView
            initialKind={catalogKind}
            onError={setError}
            onOpen={(item) => openDetails(item, "catalog")}
          />
        )}
        {view === "details" && selected && (
          <DetailsView
            key={`${selected.kind}:${selected.id}`}
            item={selected}
            busy={busy}
            onBack={() => navigate(detailsReturnView)}
            onWatch={(source) => void watch(source)}
          />
        )}
        {view === "library" && <LibraryView library={library} />}
        {view === "settings" && (
          <SettingsView
            initial={bootstrap.settings}
            configurationRequired={!hasConfiguredCatalog(bootstrap.settings)}
            onSaved={(settings) => {
              setBootstrap((current) => current && { ...current, settings });
              if (!hasConfiguredCatalog(settings)) return;
              setBusy(true);
              void refreshHome()
                .then((ready) => {
                  if (ready) navigate("home");
                })
                .catch((reason: unknown) => setError(messageOf(reason)))
                .finally(() => setBusy(false));
            }}
            onLibraryImported={setLibrary}
          />
        )}
        {view === "player" && playback && selected && (
          <PlayerView item={selected} initialStatus={playback} onClose={() => void closePlayer()} />
        )}
        {view === "diagnostics" && (
          <DiagnosticsView
            preparationId={playback?.preparation_id}
            onBack={() => navigate(playback ? "player" : "home")}
          />
        )}
      </main>
    </div>
  );
}

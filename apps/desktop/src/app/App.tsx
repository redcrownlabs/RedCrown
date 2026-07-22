import { startTransition, useCallback, useEffect, useLayoutEffect, useRef, useState } from "react";

import type { BootstrapState, CatalogPage, CatalogQuery, LibraryEpisode, LibrarySummary, MediaEpisode, MediaItem, MediaKind, PlaybackStatus, TorrentOption } from "../shared/contract.generated";
import {
  continueWatchingCandidates,
  movieIsWatched,
  nextUnwatchedEpisode,
  regularEpisodeSnapshot,
  visibleHomeItems,
} from "../features/home/home-model";
import { hasConfiguredCatalog } from "../features/settings/settings-model";
import { CatalogView } from "../features/catalog/CatalogView";
import { catalogQuery } from "../features/catalog/catalog-utils";
import { createCatalogSession } from "../features/catalog/catalog-session";
import { DetailsView } from "../features/details/DetailsView";
import { DiagnosticsView } from "../features/diagnostics/DiagnosticsView";
import { HomeView } from "../features/home/HomeView";
import type { HomeSection } from "../features/home/HomeView";
import { LibraryView } from "../features/library/LibraryView";
import { canonicalMediaId, toLibraryItem, type MediaContextRequest } from "../features/library/media-actions";
import { PlayerView } from "../features/playback/PlayerView";
import { SettingsView } from "../features/settings/SettingsView";
import { invoke, messageOf } from "../shared/ipc";
import { Icon } from "../shared/ui/Icon";
import { WindowControls } from "../shared/ui/WindowControls";
import { ContextActionPopover, type ContextAction } from "../shared/ui/ContextActionPopover";

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
  const [continueWatching, setContinueWatching] = useState<HomeSection>();
  const [view, setView] = useState<View>("home");
  const [catalogSession, setCatalogSession] = useState(() => createCatalogSession("movie"));
  const [detailsReturnView, setDetailsReturnView] = useState<View>("home");
  const [detailsEpisode, setDetailsEpisode] = useState<LibraryEpisode>();
  const [selected, setSelected] = useState<MediaItem>();
  const [playback, setPlayback] = useState<PlaybackStatus>();
  const [library, setLibrary] = useState<LibrarySummary>();
  const [error, setError] = useState<string>();
  const [busy, setBusy] = useState(true);
  const [mediaContext, setMediaContext] = useState<MediaContextRequest>();
  const mainSurfaceRef = useRef<HTMLElement>(null);
  const pendingScrollTopRef = useRef<number | undefined>(undefined);
  const watchedMovies = library?.watched_movies ?? [];
  const hideWatchedMovies = bootstrap?.settings.hide_watched_movies ?? true;
  const visibleHomeSections = [continueWatching, ...homeSections]
    .filter((section): section is HomeSection => section != null)
    .map((section) => ({
      ...section,
      items: visibleHomeItems(section.items, watchedMovies, hideWatchedMovies),
    }))
    .filter((section) => section.items.length > 0);
  const visibleFeatured = visibleHomeItems(
    bootstrap?.featured ?? [],
    watchedMovies,
    hideWatchedMovies,
  );

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
    const watchedSeries = library?.watched_series ?? [];
    const catalogItems = homeSections.flatMap((section) => section.items);
    const candidates = continueWatchingCandidates(
      catalogItems,
      watchedSeries,
      library?.favorites ?? [],
      library?.continue_watching_hidden ?? [],
    );
    if (!candidates.length) {
      setContinueWatching(undefined);
      return () => {
        active = false;
      };
    }
    void Promise.all(candidates.map(async ({ item, history }) => {
      try {
        const episodes = await invoke<MediaEpisode[]>(
          "catalog.episodes",
          { media_id: item.id },
        );
        const episode = nextUnwatchedEpisode(episodes, history);
        return episode ? { item, episode } : undefined;
      } catch {
        return undefined;
      }
    })).then((resolved) => {
      if (!active) return;
      const entries = resolved.filter((entry) => entry != null);
      if (!entries.length) {
        setContinueWatching(undefined);
        return;
      }
      setContinueWatching({
        title: "Continue watching",
        kind: "series",
        items: entries.map(({ item }) => item),
        continuations: Object.fromEntries(entries.map(({ item, episode }) => [
          mediaKey(item),
          { season: episode.season, episode: episode.episode },
        ])),
        browsable: false,
      });
    });
    return () => {
      active = false;
    };
  }, [homeSections, library]);

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

  const navigate = (next: View, scrollTop = 0) => {
    pendingScrollTopRef.current = scrollTop;
    startTransition(() => setView(next));
  };

  useLayoutEffect(() => {
    const scrollTop = pendingScrollTopRef.current;
    if (scrollTop == null || !mainSurfaceRef.current) return;
    mainSurfaceRef.current.scrollTop = scrollTop;
    pendingScrollTopRef.current = undefined;
  }, [view]);

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
    const scrollTop = catalogSession.kind === kind ? catalogSession.scrollTop : 0;
    if (catalogSession.kind !== kind) {
      setCatalogSession(createCatalogSession(kind));
    }
    navigate("catalog", scrollTop);
  }

  function openDetails(item: MediaItem, from: View, episode?: LibraryEpisode) {
    if (from === "catalog") {
      const scrollTop = mainSurfaceRef.current?.scrollTop ?? catalogSession.scrollTop;
      setCatalogSession((current) => ({ ...current, scrollTop }));
    }
    setSelected(item);
    setDetailsEpisode(episode);
    setDetailsReturnView(from);
    navigate("details");
  }

  function returnFromDetails() {
    navigate(
      detailsReturnView,
      detailsReturnView === "catalog" ? catalogSession.scrollTop : 0,
    );
  }

  async function setMovieWatched(request: MediaContextRequest, watched: boolean) {
    const summary = await invoke<LibrarySummary>("library.set_watched", {
      item: toLibraryItem(request.item),
      episodes: [],
      watched,
    });
    setLibrary(summary);
    setError(undefined);
  }

  async function markSeriesWatched(request: MediaContextRequest) {
    const episodes = await invoke<MediaEpisode[]>("catalog.episodes", {
      media_id: request.item.id,
    });
    const snapshot = regularEpisodeSnapshot(episodes);
    if (!snapshot.length) {
      throw new Error("No regular episodes are available to mark as watched.");
    }
    const summary = await invoke<LibrarySummary>("library.mark_series_watched", {
      item: toLibraryItem(request.item),
      episodes: snapshot,
    });
    setLibrary(summary);
    setError(undefined);
  }

  async function setContinueHidden(request: MediaContextRequest, hidden: boolean) {
    const summary = await invoke<LibrarySummary>("library.set_continue_watching_hidden", {
      item: toLibraryItem(request.item),
      hidden,
    });
    setLibrary(summary);
    setError(undefined);
  }

  function runMediaAction(action: () => Promise<void>) {
    return action().catch((reason: unknown) => setError(messageOf(reason)));
  }

  function contextActions(request: MediaContextRequest): ContextAction[] {
    if (request.item.kind === "movie") {
      const watched = movieIsWatched(
        {
          ...request.item,
          synopsis: "",
          genres: [],
          torrents: [],
        },
        library?.watched_movies ?? [],
      );
      return [{
        id: "watched",
        label: watched ? "Mark as unwatched" : "Mark as watched",
        onSelect: () => runMediaAction(() => setMovieWatched(request, !watched)),
      }];
    }

    const hidden = (library?.continue_watching_hidden ?? []).some(
      (id) => canonicalMediaId(id) === canonicalMediaId(request.item.id),
    );
    const actions: ContextAction[] = [{
      id: "watched",
      label: "Mark series as watched",
      onSelect: () => runMediaAction(() => markSeriesWatched(request)),
    }];
    if (request.continuation) {
      actions.push({
        id: "hide-continue",
        label: "Remove from Continue Watching",
        onSelect: () => runMediaAction(() => setContinueHidden(request, true)),
      });
    } else if (hidden) {
      actions.push({
        id: "restore-continue",
        label: "Allow in Continue Watching",
        onSelect: () => runMediaAction(() => setContinueHidden(request, false)),
      });
    }
    return actions;
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
          <button className="icon-button" onClick={() => navigate("catalog", catalogSession.scrollTop)} aria-label="Search catalog"><Icon name="search" /></button>
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

      <main className="main-surface" id="main-content" ref={mainSurfaceRef} tabIndex={-1}>
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
            onOpen={(item, episode) => openDetails(item, "home", episode)}
            onBrowse={openCatalog}
            onContext={setMediaContext}
          />
        )}
        {view === "catalog" && (
          <CatalogView
            session={catalogSession}
            onSessionChange={setCatalogSession}
            onError={setError}
            onOpen={(item) => openDetails(item, "catalog")}
            onContext={setMediaContext}
            watchedMovies={watchedMovies}
            hideWatchedMovies={hideWatchedMovies}
          />
        )}
        {view === "details" && selected && (
          <DetailsView
            key={`${selected.kind}:${selected.id}`}
            item={selected}
            busy={busy}
            initialEpisode={detailsEpisode}
            library={library}
            onBack={returnFromDetails}
            onLibraryChanged={setLibrary}
            onWatch={(source) => void watch(source)}
            onContext={setMediaContext}
          />
        )}
        {view === "library" && <LibraryView library={library} onContext={setMediaContext} />}
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
      {mediaContext && (
        <ContextActionPopover
          title={mediaContext.item.title}
          x={mediaContext.x}
          y={mediaContext.y}
          actions={contextActions(mediaContext)}
          onClose={() => setMediaContext(undefined)}
        />
      )}
    </div>
  );
}

function mediaKey(item: MediaItem) {
  return `${item.kind}:${item.id.trim().toLocaleLowerCase("en-US")}`;
}

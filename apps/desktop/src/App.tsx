import {
  startTransition,
  useCallback,
  useDeferredValue,
  useEffect,
  useId,
  useRef,
  useState,
} from "react";
import type {
  CSSProperties,
  MouseEvent as ReactMouseEvent,
  PointerEvent as ReactPointerEvent,
  ReactNode,
} from "react";

import type {
  AppSettings,
  BootstrapState,
  CatalogPage,
  CatalogQuery,
  CatalogSort,
  EndpointHealth,
  LibrarySummary,
  MediaEpisode,
  MediaItem,
  MediaKind,
  PlaybackStatus,
  PopcornImportReport,
  PopcornImportSelection,
  PopcornProfilePreview,
  SourceConfig,
  TorrentDiagnostics,
  TorrentOption,
} from "./contract.generated";
import { isCarouselDrag } from "./carousel-gesture";
import { visibleHomeItems } from "./home-model";
import {
  formatBytes,
  formatDownloadSpeed,
  playbackPercent,
  playbackStageLabel,
} from "./playback-model";
import {
  hasConfiguredCatalog,
  moveEndpoint,
  normalizeEndpoint,
  validateSource,
} from "./settings-model";
import { RedCrownPlayer } from "./RedCrownPlayer";

type View = "home" | "catalog" | "library" | "settings" | "details" | "player" | "diagnostics";
type HomeSection = {
  title: string;
  kind: MediaKind;
  items: MediaItem[];
};

const HOME_CATALOG_REQUESTS: ReadonlyArray<{ title: string; query: CatalogQuery }> = [
  { title: "Trending movies", query: catalogQuery("movie", "trending") },
  { title: "Popular series", query: catalogQuery("series", "popularity") },
  { title: "Anime right now", query: catalogQuery("anime", "trending") },
  { title: "Recently added movies", query: catalogQuery("movie", "last_added") },
  { title: "Top rated", query: catalogQuery("series", "rating") },
];

const invoke = <T,>(method: string, params: Record<string, unknown> = {}) =>
  window.redcrown.invoke<T>(method, params);

type IconName =
  | "home"
  | "library"
  | "search"
  | "settings"
  | "activity"
  | "back"
  | "play"
  | "grid"
  | "left"
  | "right"
  | "minimize"
  | "maximize"
  | "restore"
  | "close";

function Icon({ name }: { name: IconName }) {
  const paths = {
    home: <path d="M3 10.5 12 3l9 7.5V21h-6v-6H9v6H3z" />,
    library: <><path d="M5 4h14v16H5z" /><path d="M9 8h6M9 12h6M9 16h4" /></>,
    search: <><circle cx="11" cy="11" r="7" /><path d="m20 20-4-4" /></>,
    settings: <><circle cx="12" cy="12" r="3" /><path d="M19.4 15a1.7 1.7 0 0 0 .3 1.9l.1.1-2.8 2.8-.1-.1a1.7 1.7 0 0 0-1.9-.3 1.7 1.7 0 0 0-1 1.6v.2h-4V21a1.7 1.7 0 0 0-1-1.6 1.7 1.7 0 0 0-1.9.3l-.1.1L4.2 17l.1-.1a1.7 1.7 0 0 0 .3-1.9A1.7 1.7 0 0 0 3 14H2.8v-4H3a1.7 1.7 0 0 0 1.6-1 1.7 1.7 0 0 0-.3-1.9L4.2 7 7 4.2l.1.1A1.7 1.7 0 0 0 9 4.6a1.7 1.7 0 0 0 1-1.6v-.2h4V3a1.7 1.7 0 0 0 1 1.6 1.7 1.7 0 0 0 1.9-.3l.1-.1L19.8 7l-.1.1a1.7 1.7 0 0 0-.3 1.9 1.7 1.7 0 0 0 1.6 1h.2v4H21a1.7 1.7 0 0 0-1.6 1Z" /></>,
    activity: <><path d="M3 12h4l2.2-6 4.2 12 2.2-6H21" /><path d="M4 4v16h16" /></>,
    back: <path d="m15 18-6-6 6-6" />,
    play: <path d="m8 5 11 7-11 7z" />,
    grid: <><rect x="3" y="3" width="7" height="7" /><rect x="14" y="3" width="7" height="7" /><rect x="3" y="14" width="7" height="7" /><rect x="14" y="14" width="7" height="7" /></>,
    left: <path d="m15 18-6-6 6-6" />,
    right: <path d="m9 18 6-6-6-6" />,
    minimize: <path d="M5 12h14" />,
    maximize: <rect x="5" y="5" width="14" height="14" rx="1" />,
    restore: <><path d="M8 8V5h11v11h-3" /><rect x="5" y="8" width="11" height="11" rx="1" /></>,
    close: <path d="m6 6 12 12M18 6 6 18" />,
  };
  return <svg aria-hidden="true" viewBox="0 0 24 24">{paths[name]}</svg>;
}

function WindowControls() {
  const [maximized, setMaximized] = useState(false);

  useEffect(() => {
    let active = true;
    void window.redcrown.windowControls.isMaximized().then((state) => {
      if (active) setMaximized(state);
    });
    const unsubscribe = window.redcrown.windowControls.onMaximized(setMaximized);
    return () => {
      active = false;
      unsubscribe();
    };
  }, []);

  return (
    <div className="window-controls" aria-label="Window controls">
      <button onClick={() => void window.redcrown.windowControls.minimize()} aria-label="Minimize window">
        <Icon name="minimize" />
      </button>
      <button
        onClick={() => void window.redcrown.windowControls.toggleMaximize().then(setMaximized)}
        aria-label={maximized ? "Restore window" : "Maximize window"}
      >
        <Icon name={maximized ? "restore" : "maximize"} />
      </button>
      <button className="window-close" onClick={() => void window.redcrown.windowControls.close()} aria-label="Close window">
        <Icon name="close" />
      </button>
    </div>
  );
}

function HorizontalCarousel({
  label,
  children,
}: {
  label: string;
  children: ReactNode;
}) {
  const trackRef = useRef<HTMLUListElement>(null);
  const drag = useRef({ active: false, moved: false, startX: 0, startScroll: 0 });
  const [dragging, setDragging] = useState(false);
  const [canScrollLeft, setCanScrollLeft] = useState(false);
  const [canScrollRight, setCanScrollRight] = useState(true);

  useEffect(() => {
    const track = trackRef.current;
    if (!track) return;
    const update = () => {
      setCanScrollLeft(track.scrollLeft > 2);
      setCanScrollRight(track.scrollLeft + track.clientWidth < track.scrollWidth - 2);
    };
    update();
    track.addEventListener("scroll", update, { passive: true });
    const resizeObserver = new ResizeObserver(update);
    resizeObserver.observe(track);
    return () => {
      track.removeEventListener("scroll", update);
      resizeObserver.disconnect();
    };
  }, []);

  function scroll(direction: -1 | 1) {
    const track = trackRef.current;
    if (!track) return;
    track.scrollBy({ left: direction * track.clientWidth * 0.82, behavior: "smooth" });
  }

  function beginDrag(event: ReactPointerEvent<HTMLUListElement>) {
    if (event.button !== 0) return;
    drag.current = {
      active: true,
      moved: false,
      startX: event.clientX,
      startScroll: event.currentTarget.scrollLeft,
    };
  }

  function moveDrag(event: ReactPointerEvent<HTMLUListElement>) {
    if (!drag.current.active) return;
    const distance = event.clientX - drag.current.startX;
    if (!drag.current.moved && isCarouselDrag(distance)) {
      drag.current.moved = true;
      event.currentTarget.setPointerCapture(event.pointerId);
      setDragging(true);
    }
    if (!drag.current.moved) return;
    event.preventDefault();
    event.currentTarget.scrollLeft = drag.current.startScroll - distance;
  }

  function endDrag(event: ReactPointerEvent<HTMLUListElement>) {
    if (!drag.current.active) return;
    drag.current.active = false;
    if (event.currentTarget.hasPointerCapture(event.pointerId)) {
      event.currentTarget.releasePointerCapture(event.pointerId);
    }
    setDragging(false);
  }

  function suppressDraggedClick(event: ReactMouseEvent<HTMLUListElement>) {
    if (!drag.current.moved) return;
    event.preventDefault();
    event.stopPropagation();
    drag.current.moved = false;
  }

  return (
    <div className="carousel-shell">
      {canScrollLeft && (
        <button className="carousel-arrow carousel-arrow-left" onClick={() => scroll(-1)} aria-label={`Scroll ${label} left`}>
          <Icon name="left" />
        </button>
      )}
      <ul
        className={`landscape-track${dragging ? " dragging" : ""}`}
        role="list"
        ref={trackRef}
        onPointerDown={beginDrag}
        onPointerMove={moveDrag}
        onPointerUp={endDrag}
        onPointerCancel={(event) => {
          endDrag(event);
          drag.current.moved = false;
        }}
        onClickCapture={suppressDraggedClick}
      >
        {children}
      </ul>
      {canScrollRight && (
        <button className="carousel-arrow carousel-arrow-right" onClick={() => scroll(1)} aria-label={`Scroll ${label} right`}>
          <Icon name="right" />
        </button>
      )}
    </div>
  );
}

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
    if (playback) {
      await invoke("playback.cancel", {
        preparation_id: playback.preparation_id,
      }).catch(() => undefined);
    }
    setPlayback(undefined);
    navigate("details");
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

function HomeView({
  sections,
  fallback,
  busy,
  onOpen,
  onBrowse,
}: {
  sections: HomeSection[];
  fallback: MediaItem[];
  busy: boolean;
  onOpen: (item: MediaItem) => void;
  onBrowse: (kind: MediaKind) => void;
}) {
  const hero =
    sections[0]?.items.find((item) => item.torrents.length > 0) ??
    sections[0]?.items[0] ??
    fallback[0];
  return (
    <div className="home-view">
      {hero && (
        <section
          className="feature-hero"
          style={hero.backdrop_url ? { backgroundImage: `url("${hero.backdrop_url}")` } : undefined}
          aria-labelledby="feature-title"
        >
          <div className="hero-copy">
            <p className="eyebrow">Featured today</p>
            <h1 id="feature-title">{hero.title}</h1>
            <div className="hero-meta">
              <span>{hero.year ?? "New"}</span>
              {hero.rating != null && <span>★ {hero.rating.toFixed(1)}</span>}
              <span>{kindLabel(hero.kind)}</span>
            </div>
            <p>{hero.synopsis || "Discover this title from your configured catalog source."}</p>
            <div className="hero-actions">
              <button className="primary-button" onClick={() => onOpen(hero)}><Icon name="play" />View title</button>
              <button className="glass-button" onClick={() => onBrowse(hero.kind)}><Icon name="grid" />Browse {kindLabel(hero.kind).toLowerCase()}</button>
            </div>
          </div>
        </section>
      )}
      {busy && !sections.length ? (
        <div className="row-stack" aria-label="Loading catalog">
          {Array.from({ length: 3 }, (_, index) => (
            <section className="catalog-row loading-row" key={index}>
              <div className="row-title-skeleton" />
              <div className="landscape-track">
                {Array.from({ length: 6 }, (_, card) => <div className="landscape-skeleton" key={card} />)}
              </div>
            </section>
          ))}
        </div>
      ) : (
        <div className="row-stack">
          {sections.map((section) => (
            <CatalogRow
              key={`${section.kind}:${section.title}`}
              section={section}
              onOpen={onOpen}
              onBrowse={() => onBrowse(section.kind)}
            />
          ))}
        </div>
      )}
    </div>
  );
}

function CatalogRow({
  section,
  onOpen,
  onBrowse,
}: {
  section: HomeSection;
  onOpen: (item: MediaItem) => void;
  onBrowse: () => void;
}) {
  return (
    <section className="catalog-row" aria-labelledby={`row-${section.title.replaceAll(" ", "-")}`}>
      <div className="row-heading">
        <h2 id={`row-${section.title.replaceAll(" ", "-")}`}>{section.title}</h2>
        <button onClick={onBrowse}>See all <span aria-hidden="true">→</span></button>
      </div>
      <HorizontalCarousel label={section.title}>
          {section.items.slice(0, 16).map((item, index) => (
            <li key={`${item.kind}:${item.id}`}>
              <button className="landscape-card" onClick={() => onOpen(item)}>
                <div className="landscape-art">
                  <PosterImage
                    src={item.backdrop_url ?? item.poster_url}
                    fallback={item.title.slice(0, 1)}
                    loading={index < 5 ? "eager" : "lazy"}
                    fetchPriority={index < 2 ? "high" : "auto"}
                  />
                  {item.rating != null && <span className="rating-badge">★ {item.rating.toFixed(1)}</span>}
                  <span className="card-play"><Icon name="play" /></span>
                </div>
                <strong>{item.title}</strong>
                <span>{item.year ?? "Year unknown"} · {kindLabel(item.kind)}</span>
              </button>
            </li>
          ))}
      </HorizontalCarousel>
    </section>
  );
}

function CatalogView({
  initialKind,
  onError,
  onOpen,
}: {
  initialKind: MediaKind;
  onError: (message?: string) => void;
  onOpen: (item: MediaItem) => void;
}) {
  const [kind, setKind] = useState<MediaKind>(initialKind);
  const [sort, setSort] = useState<CatalogSort>("trending");
  const [genre, setGenre] = useState("");
  const [keywords, setKeywords] = useState("");
  const [items, setItems] = useState<MediaItem[]>([]);
  const [page, setPage] = useState(1);
  const [hasMore, setHasMore] = useState(true);
  const [loading, setLoading] = useState(true);
  const deferredKeywords = useDeferredValue(keywords.trim());
  const loadAnchorRef = useRef<HTMLDivElement>(null);
  const loadingRef = useRef(false);
  const queryGenerationRef = useRef(0);

  useEffect(() => {
    setKind(initialKind);
    setGenre("");
    setSort("trending");
  }, [initialKind]);

  useEffect(() => {
    let active = true;
    const generation = queryGenerationRef.current + 1;
    queryGenerationRef.current = generation;
    loadingRef.current = true;
    setLoading(true);
    setPage(1);
    void invoke<CatalogPage>("catalog.browse", {
      kind,
      page: 1,
      sort,
      genre: genre || undefined,
      keywords: deferredKeywords || undefined,
    })
      .then((result) => {
        if (!active) return;
        setItems(dedupeItems(result.items));
        setHasMore(result.has_more);
        onError(undefined);
      })
      .catch((reason: unknown) => {
        if (!active) return;
        setItems([]);
        setHasMore(false);
        onError(messageOf(reason));
      })
      .finally(() => {
        if (active && queryGenerationRef.current === generation) {
          loadingRef.current = false;
          setLoading(false);
        }
      });
    return () => {
      active = false;
    };
  }, [deferredKeywords, genre, kind, onError, sort]);

  const loadMore = useCallback(async () => {
    if (loadingRef.current || !hasMore) return;
    const generation = queryGenerationRef.current;
    const nextPage = page + 1;
    loadingRef.current = true;
    setLoading(true);
    try {
      const result = await invoke<CatalogPage>("catalog.browse", {
        kind,
        page: nextPage,
        sort,
        genre: genre || undefined,
        keywords: deferredKeywords || undefined,
      });
      if (queryGenerationRef.current !== generation) return;
      setItems((current) => dedupeItems([...current, ...result.items]));
      setPage(result.page);
      setHasMore(result.has_more);
      onError(undefined);
    } catch (reason) {
      if (queryGenerationRef.current !== generation) return;
      onError(messageOf(reason));
    } finally {
      if (queryGenerationRef.current === generation) {
        loadingRef.current = false;
        setLoading(false);
      }
    }
  }, [deferredKeywords, genre, hasMore, kind, onError, page, sort]);

  useEffect(() => {
    const anchor = loadAnchorRef.current;
    if (!anchor || !hasMore) return;
    const observer = new IntersectionObserver(
      ([entry]) => {
        if (entry?.isIntersecting) void loadMore();
      },
      { rootMargin: "600px 0px" },
    );
    observer.observe(anchor);
    return () => observer.disconnect();
  }, [hasMore, loadMore]);

  const genres = genreOptions(kind);
  return (
    <div className="catalog-view">
      <header className="catalog-header">
        <div>
          <p className="eyebrow">Explore the full catalog</p>
          <h1>{kindLabel(kind)}</h1>
        </div>
        <div className="category-tabs" aria-label="Catalog category">
          {(["movie", "series", "anime"] as const).map((option) => (
            <button className={kind === option ? "active" : ""} key={option} onClick={() => {
              setKind(option);
              setGenre("");
              setSort("trending");
            }}>{kindLabel(option)}</button>
          ))}
        </div>
      </header>
      <div className="catalog-toolbar">
        <label className="catalog-search">
          <span className="sr-only">Search {kindLabel(kind).toLowerCase()}</span>
          <Icon name="search" />
          <input type="search" value={keywords} onChange={(event) => setKeywords(event.target.value)} placeholder={`Search ${kindLabel(kind).toLowerCase()}`} />
        </label>
        <label className="compact-filter">
          <span>Sort</span>
          <select className="branded-select" aria-label="Sort catalog" value={sort} onChange={(event) => setSort(event.target.value as CatalogSort)}>
            {sortOptions(kind).map(([value, label]) => <option value={value} key={value}>{label}</option>)}
          </select>
        </label>
        {kind !== "anime" && (
          <label className="compact-filter">
            <span>Genre</span>
            <select className="branded-select" aria-label="Filter by genre" value={genre} onChange={(event) => setGenre(event.target.value)}>
              <option value="">All genres</option>
              {genres.map((option) => <option value={option.toLowerCase()} key={option}>{option}</option>)}
            </select>
          </label>
        )}
      </div>
      {loading && !items.length ? (
        <div className="poster-grid" aria-label="Loading catalog">
          {Array.from({ length: 14 }, (_, index) => <div className="poster-skeleton" key={index} />)}
        </div>
      ) : items.length ? (
        <>
          <ul className="poster-grid" role="list">
            {items.map((item, index) => (
              <li key={`${item.kind}:${item.id}`}>
                <button className="media-card" onClick={() => onOpen(item)}>
                  <div className="poster-frame">
                    <PosterImage
                      src={item.poster_url}
                      fallback={item.title.slice(0, 1)}
                      loading={index < 8 ? "eager" : "lazy"}
                      fetchPriority={index < 4 ? "high" : "auto"}
                    />
                    {item.rating != null && <span className="rating-badge">★ {item.rating.toFixed(1)}</span>}
                  </div>
                  <strong>{item.title}</strong>
                  <span>{item.year ?? "Year unknown"} · {kindLabel(item.kind)}</span>
                </button>
              </li>
            ))}
          </ul>
          {hasMore && (
            <div className={`catalog-load-anchor${loading ? " is-loading" : ""}`} ref={loadAnchorRef} aria-hidden="true">
              <span />
            </div>
          )}
        </>
      ) : (
        <div className="empty-state"><h2>No titles found</h2><p>Change the search or filters and try again.</p></div>
      )}
    </div>
  );
}

function DetailsView({
  item,
  busy,
  onBack,
  onWatch,
}: {
  item: MediaItem;
  busy: boolean;
  onBack: () => void;
  onWatch: (source: TorrentOption) => void;
}) {
  const movieSources = sortTorrents(item.torrents);
  const [episodes, setEpisodes] = useState<MediaEpisode[]>([]);
  const [episodeStatus, setEpisodeStatus] = useState<"idle" | "loading" | "ready" | "error">(
    item.kind === "movie" ? "idle" : "loading",
  );
  const [episodeError, setEpisodeError] = useState<string>();
  const [selectedSeason, setSelectedSeason] = useState<number>();
  const [selectedEpisodeKey, setSelectedEpisodeKey] = useState<string>();
  const [selectedSource, setSelectedSource] = useState(
    movieSources[0] ? torrentKey(movieSources[0]) : undefined,
  );

  useEffect(() => {
    if (item.kind === "movie") return;
    let active = true;
    void invoke<MediaEpisode[]>("catalog.episodes", { media_id: item.id })
      .then((result) => {
        if (!active) return;
        setEpisodes(result);
        const first = result.find((episode) => episode.torrents.length > 0) ?? result[0];
        setSelectedSeason(first?.season);
        setSelectedEpisodeKey(first ? episodeKey(first) : undefined);
        const firstSource = first ? sortTorrents(first.torrents)[0] : undefined;
        setSelectedSource(firstSource ? torrentKey(firstSource) : undefined);
        setEpisodeStatus("ready");
      })
      .catch((reason: unknown) => {
        if (!active) return;
        setEpisodeError(messageOf(reason));
        setEpisodeStatus("error");
      });
    return () => {
      active = false;
    };
  }, [item.id, item.kind]);

  const seasons = [...new Set(episodes.map((episode) => episode.season))];
  const seasonEpisodes = episodes.filter((episode) => episode.season === selectedSeason);
  const selectedEpisode = episodes.find((episode) => episodeKey(episode) === selectedEpisodeKey);
  const sources = item.kind === "movie"
    ? movieSources
    : sortTorrents(selectedEpisode?.torrents ?? []);
  const selectedTorrent = sources.find((source) => torrentKey(source) === selectedSource);

  function selectEpisode(episode: MediaEpisode) {
    setSelectedEpisodeKey(episodeKey(episode));
    const firstSource = sortTorrents(episode.torrents)[0];
    setSelectedSource(firstSource ? torrentKey(firstSource) : undefined);
  }

  function selectSeason(season: number) {
    setSelectedSeason(season);
    const candidates = episodes.filter((episode) => episode.season === season);
    const first = candidates.find((episode) => episode.torrents.length > 0) ?? candidates[0];
    if (first) selectEpisode(first);
  }

  return (
    <article
      className="details-view"
      style={item.backdrop_url
        ? ({ "--details-backdrop": `url("${item.backdrop_url}")` } as CSSProperties)
        : undefined}
    >
      <div className="details-shade">
        <button className="back-button" onClick={onBack}><Icon name="back" />Back</button>
        <div className="details-layout">
        <div className="details-poster">
          {item.poster_url ? (
            <PosterImage src={item.poster_url} fallback={item.title[0]} loading="eager" fetchPriority="high" />
          ) : item.title[0]}
        </div>
        <div className="details-copy">
          <p className="eyebrow">{kindLabel(item.kind)}</p>
          <h1>{item.title}</h1>
          <div className="metadata">
            <span>{item.year ?? "Year unknown"}</span>
            {item.rating != null && <span>{item.rating.toFixed(1)} / 10</span>}
            {item.kind === "movie" && sources.length > 0 && (
              <span>{sources.length} source{sources.length === 1 ? "" : "s"}</span>
            )}
          </div>
          {item.genres.length > 0 && <p className="genre-line">{item.genres.join(" · ")}</p>}
          <p className="synopsis">{item.synopsis || "No synopsis supplied by this catalog."}</p>
          {item.kind !== "movie" && (
            <section className="episode-picker" aria-label="Choose episode">
              <div className="episode-picker-heading">
                <div>
                  <span>Episodes</span>
                  {episodeStatus === "ready" && (
                    <small>{episodes.length} available</small>
                  )}
                </div>
                {seasons.length > 0 && (
                  <label>
                    <span>Season</span>
                    <select
                      className="branded-select"
                      value={selectedSeason}
                      onChange={(event) => selectSeason(Number(event.target.value))}
                    >
                      {seasons.map((season) => (
                        <option key={season} value={season}>Season {season}</option>
                      ))}
                    </select>
                  </label>
                )}
              </div>
              {episodeStatus === "loading" && <p className="supporting-note">Loading episodes…</p>}
              {episodeStatus === "error" && (
                <p className="supporting-note error-note">{episodeError}</p>
              )}
              {episodeStatus === "ready" && seasonEpisodes.length > 0 && (
                <div className="episode-list">
                  {seasonEpisodes.map((episode) => (
                    <button
                      key={episodeKey(episode)}
                      className={episodeKey(episode) === selectedEpisodeKey ? "active" : ""}
                      onClick={() => selectEpisode(episode)}
                    >
                      <strong>{episode.episode}</strong>
                      <span>
                        <b>{episode.title || `Episode ${episode.episode}`}</b>
                        <small>
                          {episode.torrents.length
                            ? `${episode.torrents.length} qualit${episode.torrents.length === 1 ? "y" : "ies"}`
                            : "No source"}
                        </small>
                      </span>
                    </button>
                  ))}
                </div>
              )}
              {selectedEpisode?.synopsis && (
                <p className="episode-synopsis">{selectedEpisode.synopsis}</p>
              )}
            </section>
          )}
          <button
            className="primary-button"
            disabled={!selectedTorrent || busy}
            onClick={() => selectedTorrent && onWatch(selectedTorrent)}
          >
            <Icon name="play" />
            {busy ? "Preparing…" : "Watch now"}
          </button>
          {episodeStatus !== "loading" && !selectedTorrent && (
            <p className="supporting-note">
              {item.kind === "movie"
                ? "No torrent source was supplied for this title."
                : "No torrent source was supplied for this episode."}
            </p>
          )}
          {sources.length > 0 && (
            <fieldset className="torrent-picker">
              <legend>Choose quality</legend>
              <div className="torrent-options">
                {sources.map((source) => (
                  <TorrentSourceChoice
                    key={torrentKey(source)}
                    source={source}
                    selected={selectedSource === torrentKey(source)}
                    onSelect={() => setSelectedSource(torrentKey(source))}
                  />
                ))}
              </div>
            </fieldset>
          )}
        </div>
      </div>
      </div>
    </article>
  );
}

function TorrentSourceChoice({
  source,
  selected,
  onSelect,
}: {
  source: TorrentOption;
  selected: boolean;
  onSelect: () => void;
}) {
  const filenameHintId = useId();
  const filename = source.file_name ?? source.file_path;
  const size = source.size_bytes != null ? formatBytes(source.size_bytes) : "Size unknown";

  return (
    <label className="torrent-option">
      <input
        type="radio"
        name="torrent-source"
        value={torrentKey(source)}
        checked={selected}
        onChange={onSelect}
        aria-describedby={filename ? filenameHintId : undefined}
      />
      <span className="torrent-option-card">
        <span className="torrent-option-heading">
          <strong>{source.quality}</strong>
          <b>{size}</b>
        </span>
        <small>
          {[
            source.provider,
            source.seeders != null ? `${source.seeders} seeders` : "Seeds unavailable",
          ].filter(Boolean).join(" · ")}
        </small>
      </span>
      {filename && (
        <>
          <span id={filenameHintId} className="sr-only">File: {filename}</span>
          <span className="torrent-filename" aria-hidden="true">{filename}</span>
        </>
      )}
    </label>
  );
}

function sortTorrents(torrents: TorrentOption[]) {
  return [...torrents].sort(
    (left, right) => (right.seeders ?? 0) - (left.seeders ?? 0),
  );
}

function torrentKey(torrent: TorrentOption) {
  return JSON.stringify([torrent.source, torrent.file_path ?? ""]);
}

function episodeKey(episode: MediaEpisode) {
  return `${episode.season}:${episode.episode}`;
}

function PlayerView({
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

function DiagnosticsView({
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
    let active = true;
    let timer: ReturnType<typeof setTimeout> | undefined;
    async function poll() {
      try {
        const next = await invoke<TorrentDiagnostics>("playback.diagnostics", {
          preparation_id: preparationId,
        });
        if (!active) return;
        setDiagnostics(next);
        setDiagnosticError(undefined);
        timer = setTimeout(() => void poll(), 1000);
      } catch (reason) {
        if (!active) return;
        setDiagnosticError(messageOf(reason));
      }
    }
    void poll();
    return () => {
      active = false;
      if (timer) clearTimeout(timer);
    };
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
    ? Math.min(100, diagnostics.pieces.verified / diagnostics.pieces.total * 100)
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
              <DiagnosticStat label="Downloaded" value={`${formatBytes(diagnostics.playback.downloaded_bytes)} / ${formatBytes(diagnostics.playback.total_bytes)}`} />
              <DiagnosticStat label="Download" value={formatDownloadSpeed(diagnostics.playback.download_mib_per_second)} />
              <DiagnosticStat label="Uploaded" value={formatBytes(diagnostics.uploaded_bytes)} />
              <DiagnosticStat label="Upload" value={formatDownloadSpeed(diagnostics.upload_mib_per_second)} />
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
              <DiagnosticStat label="Verified" value={diagnostics.pieces.verified} />
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

function LibraryView({ library }: { library?: LibrarySummary }) {
  if (!library) {
    return <div className="empty-state"><p>Loading library…</p></div>;
  }
  return (
    <div className="library-view">
      <header className="compact-header">
        <div><p className="eyebrow">Your library</p><h1>Favorites and watched</h1></div>
        <div className="library-stats" aria-label="Library totals">
          <span><strong>{library.favorite_count}</strong>Favorites</span>
          <span><strong>{library.watched_movie_count}</strong>Movies watched</span>
          <span><strong>{library.watched_episode_count}</strong>Episodes watched</span>
        </div>
      </header>
      <section aria-labelledby="favorites-heading">
        <div className="section-heading">
          <h2 id="favorites-heading">Favorites</h2>
          <span>{library.favorite_count} saved</span>
        </div>
        {library.favorites.length ? (
          <div className="poster-grid">
            {library.favorites.map((item) => (
              <article className="media-card library-card" key={`${item.kind}:${item.external_id}`}>
                <div className="poster-frame">
                  {item.poster_url ? (
                    <PosterImage src={item.poster_url} fallback={(item.title ?? item.external_id)[0]} loading="lazy" />
                  ) : <span className="poster-fallback">{(item.title ?? item.external_id)[0]}</span>}
                </div>
                <strong>{item.title ?? item.external_id}</strong>
                <span>{item.year ?? "Year unknown"} · {item.kind === "movie" ? "Movie" : "Series"}</span>
              </article>
            ))}
          </div>
        ) : (
          <div className="empty-state library-empty">
            <h3>No favorites yet</h3>
            <p>Import a Popcorn Time profile from Settings or save titles in RedCrown.</p>
          </div>
        )}
      </section>
    </div>
  );
}

function SettingsView({
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

function PopcornMigration({
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

function catalogQuery(kind: MediaKind, sort: CatalogSort): CatalogQuery {
  return { kind, page: 1, sort };
}

function dedupeItems(items: MediaItem[]) {
  const seen = new Set<string>();
  return items.filter((item) => {
    const key = `${item.kind}:${item.id}`;
    if (seen.has(key)) return false;
    seen.add(key);
    return true;
  });
}

function kindLabel(kind: MediaKind) {
  switch (kind) {
    case "movie":
      return "Movies";
    case "series":
      return "Series";
    case "anime":
      return "Anime";
  }
}

function sortOptions(kind: MediaKind): Array<[CatalogSort, string]> {
  if (kind === "movie") {
    return [
      ["trending", "Trending"],
      ["popularity", "Popular"],
      ["last_added", "Recently added"],
      ["year", "Newest year"],
      ["title", "Title"],
      ["rating", "Highest rated"],
    ];
  }
  return [
    ["trending", "Trending"],
    ["popularity", "Popular"],
    ["updated", "Recently updated"],
    ["year", "Newest year"],
    ["title", "Name"],
    ["rating", "Highest rated"],
  ];
}

function genreOptions(kind: MediaKind) {
  return kind === "movie"
    ? ["Action", "Adventure", "Animation", "Comedy", "Crime", "Documentary", "Drama", "Family", "Fantasy", "History", "Horror", "Music", "Mystery", "Romance", "Science Fiction", "Thriller", "War", "Western"]
    : ["Action & Adventure", "Animation", "Comedy", "Crime", "Documentary", "Drama", "Family", "Kids", "Mystery", "Reality", "Romance", "Sci-Fi & Fantasy", "Talk", "War & Politics", "Western"];
}

function messageOf(reason: unknown) {
  return reason instanceof Error ? reason.message : "Something went wrong";
}

function PosterImage({
  src,
  fallback,
  loading,
  fetchPriority = "auto",
}: {
  src?: string;
  fallback: string;
  loading: "eager" | "lazy";
  fetchPriority?: "high" | "low" | "auto";
}) {
  const [failedSrc, setFailedSrc] = useState<string>();
  if (!src || failedSrc === src) {
    return <span className="poster-fallback" aria-hidden="true">{fallback}</span>;
  }
  return (
    <img
      src={src}
      alt=""
      width="360"
      height="540"
      loading={loading}
      fetchPriority={fetchPriority}
      decoding="async"
      onError={() => setFailedSrc(src)}
    />
  );
}

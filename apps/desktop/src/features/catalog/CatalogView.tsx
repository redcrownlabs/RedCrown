import { useCallback, useDeferredValue, useEffect, useRef, useState } from "react";
import type { CatalogPage, CatalogSort, LibraryItem, MediaItem } from "../../shared/contract.generated";
import { invoke, messageOf } from "../../shared/ipc";
import { Icon } from "../../shared/ui/Icon";
import { PosterImage } from "../../shared/ui/PosterImage";
import { dedupeItems, genreOptions, kindLabel, sortOptions } from "./catalog-utils";
import { actionItemFromMedia, filterWatchedMovies, type MediaContextRequest } from "../library/media-actions";
import {
  catalogQueryKey,
  createCatalogSession,
  type CatalogSession,
  type CatalogSessionUpdate,
} from "./catalog-session";

export function CatalogView({
  session,
  onSessionChange,
  onError,
  onOpen,
  onContext,
  watchedMovies,
  hideWatchedMovies,
}: {
  session: CatalogSession;
  onSessionChange: (update: CatalogSessionUpdate) => void;
  onError: (message?: string) => void;
  onOpen: (item: MediaItem) => void;
  onContext: (request: MediaContextRequest) => void;
  watchedMovies: LibraryItem[];
  hideWatchedMovies: boolean;
}) {
  const { kind, sort, genre, keywords, items, page, hasMore } = session;
  const deferredKeywords = useDeferredValue(keywords.trim());
  const queryKey = catalogQueryKey(session, deferredKeywords);
  const [loading, setLoading] = useState(session.loadedQueryKey !== queryKey);
  const loadAnchorRef = useRef<HTMLDivElement>(null);
  const loadingRef = useRef(false);
  const queryGenerationRef = useRef(0);

  useEffect(() => {
    if (session.loadedQueryKey === queryKey) {
      loadingRef.current = false;
      setLoading(false);
      return;
    }
    let active = true;
    const generation = queryGenerationRef.current + 1;
    queryGenerationRef.current = generation;
    loadingRef.current = true;
    setLoading(true);
    void invoke<CatalogPage>("catalog.browse", {
      kind,
      page: 1,
      sort,
      genre: genre || undefined,
      keywords: deferredKeywords || undefined,
    })
      .then((result) => {
        if (!active) return;
        onSessionChange((current) =>
          catalogQueryKey(current, current.keywords.trim()) === queryKey
            ? {
                ...current,
                items: dedupeItems(result.items),
                page: result.page,
                hasMore: result.has_more,
                loadedQueryKey: queryKey,
              }
            : current,
        );
        onError(undefined);
      })
      .catch((reason: unknown) => {
        if (!active) return;
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
  }, [deferredKeywords, genre, kind, onError, onSessionChange, queryKey, session.loadedQueryKey, sort]);

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
      onSessionChange((current) =>
        current.loadedQueryKey === queryKey
          ? {
              ...current,
              items: dedupeItems([...current.items, ...result.items]),
              page: result.page,
              hasMore: result.has_more,
            }
          : current,
      );
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
  }, [deferredKeywords, genre, hasMore, kind, onError, onSessionChange, page, queryKey, sort]);

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
  }, [hasMore, loadMore, loading]);

  const genres = genreOptions(kind);
  const visibleItems = filterWatchedMovies(items, watchedMovies, hideWatchedMovies);
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
              onSessionChange(() => createCatalogSession(option));
            }}>{kindLabel(option)}</button>
          ))}
        </div>
      </header>
      <div className="catalog-toolbar">
        <label className="catalog-search">
          <span className="sr-only">Search {kindLabel(kind).toLowerCase()}</span>
          <Icon name="search" />
          <input type="search" value={keywords} onChange={(event) => {
            const value = event.target.value;
            onSessionChange((current) => ({ ...current, keywords: value }));
          }} placeholder={`Search ${kindLabel(kind).toLowerCase()}`} />
        </label>
        <label className="compact-filter">
          <span>Sort</span>
          <select className="branded-select" aria-label="Sort catalog" value={sort} onChange={(event) => {
            const value = event.target.value as CatalogSort;
            onSessionChange((current) => ({ ...current, sort: value }));
          }}>
            {sortOptions(kind).map(([value, label]) => <option value={value} key={value}>{label}</option>)}
          </select>
        </label>
        {kind !== "anime" && (
          <label className="compact-filter">
            <span>Genre</span>
            <select className="branded-select" aria-label="Filter by genre" value={genre} onChange={(event) => {
              const value = event.target.value;
              onSessionChange((current) => ({ ...current, genre: value }));
            }}>
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
      ) : visibleItems.length || hasMore ? (
        <>
          <ul className="poster-grid" role="list">
            {visibleItems.map((item, index) => (
              <li key={`${item.kind}:${item.id}`}>
                <button
                  className="media-card"
                  onClick={() => onOpen(item)}
                  onContextMenu={(event) => {
                    event.preventDefault();
                    onContext({
                      item: actionItemFromMedia(item),
                      x: event.clientX,
                      y: event.clientY,
                      continuation: false,
                    });
                  }}
                >
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
        <div className="empty-state">
          <h2>{items.length ? "All loaded movies are watched" : "No titles found"}</h2>
          <p>{items.length
            ? "Turn off Hide watched movies in Settings to show them."
            : "Change the search or filters and try again."}</p>
        </div>
      )}
    </div>
  );
}

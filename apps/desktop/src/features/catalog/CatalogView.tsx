import { useCallback, useDeferredValue, useEffect, useRef, useState } from "react";
import type { CatalogPage, CatalogSort, LibraryItem, MediaItem, MediaKind } from "../../shared/contract.generated";
import { invoke, messageOf } from "../../shared/ipc";
import { Icon } from "../../shared/ui/Icon";
import { PosterImage } from "../../shared/ui/PosterImage";
import { dedupeItems, genreOptions, kindLabel, sortOptions } from "./catalog-utils";
import { actionItemFromMedia, filterWatchedMovies, type MediaContextRequest } from "../library/media-actions";

export function CatalogView({
  initialKind,
  onError,
  onOpen,
  onContext,
  watchedMovies,
  hideWatchedMovies,
}: {
  initialKind: MediaKind;
  onError: (message?: string) => void;
  onOpen: (item: MediaItem) => void;
  onContext: (request: MediaContextRequest) => void;
  watchedMovies: LibraryItem[];
  hideWatchedMovies: boolean;
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

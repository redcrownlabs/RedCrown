import type { LibrarySummary } from "../../shared/contract.generated";
import { PosterImage } from "../../shared/ui/PosterImage";
import { actionItemFromLibrary, type MediaContextRequest } from "./media-actions";

export function LibraryView({
  library,
  onContext,
}: {
  library?: LibrarySummary;
  onContext: (request: MediaContextRequest) => void;
}) {
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
              <article
                className="media-card library-card"
                key={`${item.kind}:${item.external_id}`}
                onContextMenu={(event) => {
                  event.preventDefault();
                  onContext({
                    item: actionItemFromLibrary(item),
                    x: event.clientX,
                    y: event.clientY,
                    continuation: false,
                  });
                }}
              >
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

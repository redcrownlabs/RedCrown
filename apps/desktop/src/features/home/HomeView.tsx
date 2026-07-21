import type { LibraryEpisode, MediaItem, MediaKind } from "../../shared/contract.generated";
import { HorizontalCarousel } from "../../shared/ui/HorizontalCarousel";
import { Icon } from "../../shared/ui/Icon";
import { PosterImage } from "../../shared/ui/PosterImage";
import { kindLabel } from "../catalog/catalog-utils";
import { continuationLabel } from "./home-model";
import { actionItemFromMedia, type MediaContextRequest } from "../library/media-actions";

export type HomeSection = {
  title: string;
  kind: MediaKind;
  items: MediaItem[];
  continuations?: Record<string, LibraryEpisode>;
  browsable?: boolean;
};

export function HomeView({
  sections,
  fallback,
  busy,
  onOpen,
  onBrowse,
  onContext,
}: {
  sections: HomeSection[];
  fallback: MediaItem[];
  busy: boolean;
  onOpen: (item: MediaItem, episode?: LibraryEpisode) => void;
  onBrowse: (kind: MediaKind) => void;
  onContext: (request: MediaContextRequest) => void;
}) {
  const discoveryItems = sections.find((section) => section.browsable !== false)?.items ?? [];
  const hero =
    discoveryItems.find((item) => item.torrents.length > 0) ??
    discoveryItems[0] ??
    fallback[0];
  return (
    <div className="home-view">
      {hero && (
        <section
          className="feature-hero"
          style={hero.backdrop_url ? { backgroundImage: `url("${hero.backdrop_url}")` } : undefined}
          aria-labelledby="feature-title"
          onContextMenu={(event) => {
            event.preventDefault();
            onContext({
              item: actionItemFromMedia(hero),
              x: event.clientX,
              y: event.clientY,
              continuation: false,
            });
          }}
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
              onBrowse={section.browsable === false ? undefined : () => onBrowse(section.kind)}
              onContext={onContext}
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
  onContext,
}: {
  section: HomeSection;
  onOpen: (item: MediaItem, episode?: LibraryEpisode) => void;
  onBrowse?: () => void;
  onContext: (request: MediaContextRequest) => void;
}) {
  return (
    <section className="catalog-row" aria-labelledby={`row-${section.title.replaceAll(" ", "-")}`}>
      <div className="row-heading">
        <h2 id={`row-${section.title.replaceAll(" ", "-")}`}>{section.title}</h2>
        {onBrowse && <button onClick={onBrowse}>See all <span aria-hidden="true">→</span></button>}
      </div>
      <HorizontalCarousel label={section.title}>
          {section.items.slice(0, 16).map((item, index) => {
            const continuation = section.continuations?.[mediaKey(item)];
            return (
              <li key={`${item.kind}:${item.id}`}>
                <button
                  className="landscape-card"
                  onClick={() => onOpen(item, continuation)}
                  onContextMenu={(event) => {
                    event.preventDefault();
                    onContext({
                      item: actionItemFromMedia(item),
                      x: event.clientX,
                      y: event.clientY,
                      continuation: continuation != null,
                    });
                  }}
                >
                  <div className="landscape-art">
                    <PosterImage
                      src={item.backdrop_url ?? item.poster_url}
                      fallback={item.title.slice(0, 1)}
                      loading={index < 5 ? "eager" : "lazy"}
                      fetchPriority={index < 2 ? "high" : "auto"}
                    />
                    {item.rating != null && <span className="rating-badge">★ {item.rating.toFixed(1)}</span>}
                    {continuation && (
                      <span className="continue-badge">
                        S{continuation.season} E{continuation.episode}
                      </span>
                    )}
                    <span className="card-play"><Icon name="play" /></span>
                  </div>
                  <strong>{item.title}</strong>
                  <span>{continuation
                    ? continuationLabel(continuation)
                    : `${item.year ?? "Year unknown"} · ${kindLabel(item.kind)}`}</span>
                </button>
              </li>
            );
          })}
      </HorizontalCarousel>
    </section>
  );
}

function mediaKey(item: MediaItem) {
  return `${item.kind}:${item.id.trim().toLocaleLowerCase("en-US")}`;
}

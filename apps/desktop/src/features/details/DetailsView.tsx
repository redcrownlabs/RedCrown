import { useEffect, useId, useState } from "react";
import type { CSSProperties } from "react";
import type {
  LibraryEpisode,
  LibrarySummary,
  MediaEpisode,
  MediaItem,
  TorrentOption,
} from "../../shared/contract.generated";
import { formatBytes } from "../playback/playback-model";
import {
  episodeIsWatched,
  movieIsWatched,
} from "../home/home-model";
import { invoke, messageOf } from "../../shared/ipc";
import { Icon } from "../../shared/ui/Icon";
import { PosterImage } from "../../shared/ui/PosterImage";
import { kindLabel } from "../catalog/catalog-utils";
import { actionItemFromMedia, type MediaContextRequest } from "../library/media-actions";

export function DetailsView({
  item,
  busy,
  initialEpisode,
  library,
  onBack,
  onLibraryChanged,
  onWatch,
  onContext,
}: {
  item: MediaItem;
  busy: boolean;
  initialEpisode?: LibraryEpisode;
  library?: LibrarySummary;
  onBack: () => void;
  onLibraryChanged: (library: LibrarySummary) => void;
  onWatch: (source: TorrentOption) => void;
  onContext: (request: MediaContextRequest) => void;
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
  const [savingWatched, setSavingWatched] = useState(false);
  const [watchedError, setWatchedError] = useState<string>();
  const initialSeason = initialEpisode?.season;
  const initialEpisodeNumber = initialEpisode?.episode;

  useEffect(() => {
    if (item.kind === "movie") return;
    let active = true;
    void invoke<MediaEpisode[]>("catalog.episodes", { media_id: item.id })
      .then((result) => {
        if (!active) return;
        setEpisodes(result);
        const requested = initialSeason != null && initialEpisodeNumber != null
          ? result.find((episode) =>
              episode.season === initialSeason
              && episode.episode === initialEpisodeNumber)
          : undefined;
        const first = requested
          ?? result.find((episode) => episode.torrents.length > 0)
          ?? result[0];
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
  }, [initialEpisodeNumber, initialSeason, item.id, item.kind]);

  const seasons = [...new Set(episodes.map((episode) => episode.season))];
  const seasonEpisodes = episodes.filter((episode) => episode.season === selectedSeason);
  const selectedEpisode = episodes.find((episode) => episodeKey(episode) === selectedEpisodeKey);
  const sources = item.kind === "movie"
    ? movieSources
    : sortTorrents(selectedEpisode?.torrents ?? []);
  const selectedTorrent = sources.find((source) => torrentKey(source) === selectedSource);
  const watched = item.kind === "movie"
    ? movieIsWatched(item, library?.watched_movies ?? [])
    : selectedEpisode != null && episodeIsWatched(
      item,
      selectedEpisode,
      library?.watched_series ?? [],
    );

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

  async function toggleWatched() {
    if (item.kind !== "movie" && !selectedEpisode) return;
    const episodeSelection = selectedEpisode
      ? [{ season: selectedEpisode.season, episode: selectedEpisode.episode }]
      : [];
    setSavingWatched(true);
    setWatchedError(undefined);
    try {
      const summary = await invoke<LibrarySummary>("library.set_watched", {
        item: {
          external_id: item.id,
          kind: item.kind === "movie" ? "movie" : "series",
          title: item.title,
          year: item.year,
          poster_url: item.poster_url,
        },
        episodes: item.kind === "movie" ? [] : episodeSelection,
        watched: !watched,
      });
      onLibraryChanged(summary);
    } catch (reason) {
      setWatchedError(messageOf(reason));
    } finally {
      setSavingWatched(false);
    }
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
        <div
          className="details-poster"
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
          {item.poster_url ? (
            <PosterImage src={item.poster_url} fallback={item.title[0]} loading="eager" fetchPriority="high" />
          ) : item.title[0]}
        </div>
        <div className="details-copy">
          <p className="eyebrow">{kindLabel(item.kind)}</p>
          <h1
            onContextMenu={(event) => {
              event.preventDefault();
              onContext({
                item: actionItemFromMedia(item),
                x: event.clientX,
                y: event.clientY,
                continuation: false,
              });
            }}
          >{item.title}</h1>
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
          <div className="details-actions">
            <button
              className="primary-button"
              disabled={!selectedTorrent || busy}
              onClick={() => selectedTorrent && onWatch(selectedTorrent)}
            >
              <Icon name="play" />
              {busy ? "Preparing…" : "Watch now"}
            </button>
            <button
              className="secondary-button"
              disabled={savingWatched || (item.kind !== "movie" && !selectedEpisode)}
              aria-pressed={watched}
              onClick={() => void toggleWatched()}
            >
              {savingWatched
                ? "Saving…"
                : item.kind === "movie"
                  ? watched ? "Watched · Undo" : "Mark as watched"
                  : watched ? "Episode watched · Undo" : "Mark episode watched"}
            </button>
          </div>
          {watchedError && <p className="supporting-note error-note" role="alert">{watchedError}</p>}
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

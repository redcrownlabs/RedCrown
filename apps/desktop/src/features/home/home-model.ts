import type {
  LibraryEpisode,
  LibraryItem,
  MediaEpisode,
  MediaItem,
  WatchedSeries,
} from "../../shared/contract.generated";
import { filterWatchedMovies } from "../library/media-actions";

export type ContinueWatchingEntry = {
  item: MediaItem;
  episode: MediaEpisode;
};

export type ContinueWatchingCandidate = {
  item: MediaItem;
  history: WatchedSeries;
};

/** Removes watched movies while preserving every non-movie catalog item. */
export function visibleHomeItems(
  items: MediaItem[],
  watchedMovies: LibraryItem[],
  hideWatchedMovies = true,
) {
  return filterWatchedMovies(items, watchedMovies, hideWatchedMovies);
}

/** Builds resolvable series candidates without inventing missing display metadata. */
export function continueWatchingCandidates(
  catalogItems: MediaItem[],
  watchedSeries: WatchedSeries[],
  libraryItems: LibraryItem[] = [],
  hiddenSeries: string[] = [],
): ContinueWatchingCandidate[] {
  const hiddenIds = new Set(hiddenSeries.map(canonicalMediaId));
  const catalogById = new Map<string, MediaItem>();
  for (const item of catalogItems) {
    if (item.kind !== "movie") catalogById.set(canonicalMediaId(item.id), item);
  }
  const libraryById = new Map(
    libraryItems
      .filter((item) => item.kind === "series")
      .map((item) => [canonicalMediaId(item.external_id), item]),
  );
  return watchedSeries.flatMap((history) => {
    if (hiddenIds.has(canonicalMediaId(history.external_id))) return [];
    const item = catalogById.get(canonicalMediaId(history.external_id))
      ?? mediaFromHistory(history, libraryById.get(canonicalMediaId(history.external_id)));
    return item ? [{ item, history }] : [];
  });
}

/** Selects the first watchable regular episode not explicitly marked watched. */
export function nextUnwatchedEpisode(
  episodes: MediaEpisode[],
  history: WatchedSeries,
): MediaEpisode | undefined {
  const watched = new Set(
    history.episodes.map((episode) => `${episode.season}:${episode.episode}`),
  );
  return [...episodes]
    .filter((episode) =>
      episode.season > 0
      && episode.episode > 0
      && episode.torrents.length > 0
      && !watched.has(`${episode.season}:${episode.episode}`))
    .sort(compareEpisodes)[0];
}

export function regularEpisodeSnapshot(episodes: MediaEpisode[]): LibraryEpisode[] {
  const unique = new Map<string, LibraryEpisode>();
  for (const episode of episodes) {
    if (episode.season <= 0 || episode.episode <= 0) continue;
    unique.set(`${episode.season}:${episode.episode}`, {
      season: episode.season,
      episode: episode.episode,
    });
  }
  return [...unique.values()].sort(compareEpisodes);
}

export function continuationLabel(episode: LibraryEpisode) {
  return `Season ${episode.season} · Episode ${episode.episode}`;
}

export function movieIsWatched(item: MediaItem, watchedMovies: LibraryItem[]) {
  const id = canonicalMediaId(item.id);
  return watchedMovies.some((watched) => canonicalMediaId(watched.external_id) === id);
}

export function episodeIsWatched(
  item: MediaItem,
  episode: Pick<MediaEpisode, "season" | "episode">,
  watchedSeries: WatchedSeries[],
) {
  const history = watchedSeries.find(
    (candidate) => canonicalMediaId(candidate.external_id) === canonicalMediaId(item.id),
  );
  return history?.episodes.some(
    (watched) => watched.season === episode.season && watched.episode === episode.episode,
  ) ?? false;
}

export function seriesIsCaughtUp(
  item: MediaItem,
  episodes: MediaEpisode[],
  watchedSeries: WatchedSeries[],
) {
  const history = watchedSeries.find(
    (candidate) => canonicalMediaId(candidate.external_id) === canonicalMediaId(item.id),
  );
  if (!history) return false;
  const regular = regularEpisodeSnapshot(episodes);
  const watched = new Set(
    history.episodes.map((episode) => `${episode.season}:${episode.episode}`),
  );
  return regular.length > 0 && regular.every(
    (episode) => watched.has(`${episode.season}:${episode.episode}`),
  );
}

function mediaFromHistory(history: WatchedSeries, libraryItem?: LibraryItem): MediaItem | undefined {
  const title = history.title ?? libraryItem?.title;
  if (!title) return undefined;
  return {
    id: history.external_id,
    title,
    year: history.year ?? libraryItem?.year,
    synopsis: "",
    poster_url: history.poster_url ?? libraryItem?.poster_url,
    kind: "series",
    genres: [],
    torrents: [],
  };
}

function compareEpisodes(
  left: Pick<MediaEpisode | LibraryEpisode, "season" | "episode">,
  right: Pick<MediaEpisode | LibraryEpisode, "season" | "episode">,
) {
  return left.season - right.season || left.episode - right.episode;
}

function canonicalMediaId(value: string) {
  return value.trim().toLocaleLowerCase("en-US");
}

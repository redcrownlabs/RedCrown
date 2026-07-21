import type { LibraryItem, MediaItem } from "../../shared/contract.generated";

export type MediaActionItem = {
  id: string;
  kind: "movie" | "series";
  title: string;
  year?: number;
  poster_url?: string;
};

export type MediaContextRequest = {
  item: MediaActionItem;
  x: number;
  y: number;
  continuation: boolean;
};

export function actionItemFromMedia(item: MediaItem): MediaActionItem {
  return {
    id: item.id,
    kind: item.kind === "movie" ? "movie" : "series",
    title: item.title,
    year: item.year,
    poster_url: item.poster_url,
  };
}

export function actionItemFromLibrary(item: LibraryItem): MediaActionItem {
  return {
    id: item.external_id,
    kind: item.kind,
    title: item.title ?? item.external_id,
    year: item.year,
    poster_url: item.poster_url,
  };
}

export function toLibraryItem(item: MediaActionItem): LibraryItem {
  return {
    external_id: item.id,
    kind: item.kind,
    title: item.title,
    year: item.year,
    poster_url: item.poster_url,
  };
}

export function canonicalMediaId(value: string) {
  return value.trim().toLocaleLowerCase("en-US");
}

/** Omits watched movies without changing series, anime, or the source arrays. */
export function filterWatchedMovies<T extends Pick<MediaItem, "id" | "kind">>(
  items: T[],
  watchedMovies: LibraryItem[],
  hidden: boolean,
): T[] {
  if (!hidden || watchedMovies.length === 0) return items;
  const watchedIds = new Set(
    watchedMovies.map((item) => canonicalMediaId(item.external_id)),
  );
  return items.filter(
    (item) => item.kind !== "movie" || !watchedIds.has(canonicalMediaId(item.id)),
  );
}

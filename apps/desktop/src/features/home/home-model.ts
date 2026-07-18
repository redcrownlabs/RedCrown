import type { LibraryItem, MediaItem } from "../../shared/contract.generated";

/** Removes watched movies while preserving every non-movie catalog item. */
export function visibleHomeItems(items: MediaItem[], watchedMovies: LibraryItem[]) {
  const watchedIds = new Set(
    watchedMovies.map((item) => canonicalMediaId(item.external_id)),
  );
  return items.filter(
    (item) => item.kind !== "movie" || !watchedIds.has(canonicalMediaId(item.id)),
  );
}

function canonicalMediaId(value: string) {
  return value.trim().toLocaleLowerCase("en-US");
}

import type { CatalogQuery, CatalogSort, MediaItem, MediaKind } from "../../shared/contract.generated";

export function catalogQuery(kind: MediaKind, sort: CatalogSort): CatalogQuery {
  return { kind, page: 1, sort };
}

export function dedupeItems(items: MediaItem[]) {
  const seen = new Set<string>();
  return items.filter((item) => {
    const key = `${item.kind}:${item.id}`;
    if (seen.has(key)) return false;
    seen.add(key);
    return true;
  });
}

export function kindLabel(kind: MediaKind) {
  switch (kind) {
    case "movie":
      return "Movies";
    case "series":
      return "Series";
    case "anime":
      return "Anime";
  }
}

export function sortOptions(kind: MediaKind): Array<[CatalogSort, string]> {
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

export function genreOptions(kind: MediaKind) {
  return kind === "movie"
    ? ["Action", "Adventure", "Animation", "Comedy", "Crime", "Documentary", "Drama", "Family", "Fantasy", "History", "Horror", "Music", "Mystery", "Romance", "Science Fiction", "Thriller", "War", "Western"]
    : ["Action & Adventure", "Animation", "Comedy", "Crime", "Documentary", "Drama", "Family", "Kids", "Mystery", "Reality", "Romance", "Sci-Fi & Fantasy", "Talk", "War & Politics", "Western"];
}

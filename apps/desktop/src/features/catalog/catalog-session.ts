import type {
  CatalogSort,
  MediaItem,
  MediaKind,
} from "../../shared/contract.generated";

/** Durable state for one catalog drill-down/back navigation session. */
export type CatalogSession = {
  kind: MediaKind;
  sort: CatalogSort;
  genre: string;
  keywords: string;
  items: MediaItem[];
  page: number;
  hasMore: boolean;
  loadedQueryKey?: string;
  scrollTop: number;
};

export type CatalogSessionUpdate = (current: CatalogSession) => CatalogSession;

export function createCatalogSession(kind: MediaKind): CatalogSession {
  return {
    kind,
    sort: "trending",
    genre: "",
    keywords: "",
    items: [],
    page: 1,
    hasMore: true,
    scrollTop: 0,
  };
}

export function catalogQueryKey(
  session: Pick<CatalogSession, "kind" | "sort" | "genre">,
  keywords: string,
) {
  return JSON.stringify([
    session.kind,
    session.sort,
    session.genre,
    keywords,
  ]);
}

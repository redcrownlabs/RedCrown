import { describe, expect, it } from "vitest";

import type { LibraryItem, MediaItem } from "../../shared/contract.generated";
import { visibleHomeItems } from "./home-model";

const media = (id: string, kind: MediaItem["kind"]): MediaItem => ({
  id,
  kind,
  title: id,
  synopsis: "",
  genres: [],
  torrents: [],
});

describe("visibleHomeItems", () => {
  it("removes watched movies without hiding series or unwatched movies", () => {
    const watched: LibraryItem[] = [{ external_id: " TT-WATCHED ", kind: "movie" }];

    expect(
      visibleHomeItems(
        [media("tt-watched", "movie"), media("tt-fresh", "movie"), media("tt-watched", "series")],
        watched,
      ).map((item) => `${item.kind}:${item.id}`),
    ).toEqual(["movie:tt-fresh", "series:tt-watched"]);
  });
});

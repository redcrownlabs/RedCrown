import { describe, expect, it } from "vitest";

import type { LibraryItem, MediaEpisode, MediaItem, WatchedSeries } from "../../shared/contract.generated";
import {
  continueWatchingCandidates,
  continuationLabel,
  episodeIsWatched,
  nextUnwatchedEpisode,
  regularEpisodeSnapshot,
  seriesIsCaughtUp,
  visibleHomeItems,
} from "./home-model";

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

  it("keeps watched movies when the global preference is disabled", () => {
    const items = [media("tt-watched", "movie")];
    const watched: LibraryItem[] = [{ external_id: "tt-watched", kind: "movie" }];

    expect(visibleHomeItems(items, watched, false)).toBe(items);
  });
});

const history = (overrides: Partial<WatchedSeries> = {}): WatchedSeries => ({
  external_id: "tt-series",
  title: "Series",
  latest_season: 1,
  latest_episode: 2,
  episodes: [{ season: 1, episode: 1 }, { season: 1, episode: 2 }],
  ...overrides,
});

const episode = (season: number, number: number, watchable = true): MediaEpisode => ({
  season,
  episode: number,
  title: `S${season}E${number}`,
  synopsis: "",
  torrents: watchable ? [{ quality: "1080p", source: `magnet:${season}:${number}` }] : [],
});

describe("continue watching", () => {
  it("excludes series explicitly removed from Continue Watching", () => {
    expect(continueWatchingCandidates([], [history()], [], [" TT-SERIES "])).toEqual([]);
  });

  it("uses current catalog metadata and selects the first newer watchable episode", () => {
    const catalogItem = media("TT-SERIES", "series");
    const candidates = continueWatchingCandidates([catalogItem], [history()]);
    const next = nextUnwatchedEpisode(
      [episode(1, 1), episode(1, 2), episode(1, 3, false), episode(2, 1)],
      candidates[0].history,
    );

    expect(candidates[0].item).toBe(catalogItem);
    expect(next && [next.season, next.episode]).toEqual([2, 1]);
  });

  it("can restore a catalog candidate from imported metadata", () => {
    const [candidate] = continueWatchingCandidates([], [history({ poster_url: "https://img.test/a.jpg" })]);

    expect(candidate.item).toMatchObject({
      id: "tt-series",
      title: "Series",
      kind: "series",
      poster_url: "https://img.test/a.jpg",
    });
  });

  it("does not fabricate a title when neither library nor catalog has metadata", () => {
    expect(continueWatchingCandidates([], [history({ title: undefined })])).toEqual([]);
  });

  it("uses favorite metadata when legacy watched history has identity only", () => {
    const [candidate] = continueWatchingCandidates(
      [],
      [history({ title: undefined })],
      [{ external_id: "TT-SERIES", kind: "series", title: "Favorite title", year: 2024 }],
    );

    expect(candidate.item).toMatchObject({ title: "Favorite title", year: 2024 });
  });

  it("treats specials as outside the regular watched snapshot", () => {
    expect(regularEpisodeSnapshot([episode(0, 1), episode(1, 1), episode(1, 1)])).toEqual([
      { season: 1, episode: 1 },
    ]);
  });

  it("is caught up only through the latest currently known regular episode", () => {
    const item = media("tt-series", "series");
    expect(seriesIsCaughtUp(item, [episode(1, 1), episode(1, 2)], [history()])).toBe(true);
    expect(seriesIsCaughtUp(item, [episode(1, 1), episode(1, 3)], [history()])).toBe(false);
  });

  it("matches watched state only for the selected episode", () => {
    const item = media("tt-series", "series");

    expect(episodeIsWatched(item, episode(1, 2), [history()])).toBe(true);
    expect(episodeIsWatched(item, episode(1, 1), [history()])).toBe(true);
    expect(episodeIsWatched(item, episode(1, 3), [history()])).toBe(false);
  });

  it("labels the exact episode offered on the home card", () => {
    expect(continuationLabel({ season: 3, episode: 1 })).toBe("Season 3 · Episode 1");
  });
});

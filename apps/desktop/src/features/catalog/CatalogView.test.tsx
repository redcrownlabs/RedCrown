/** @vitest-environment jsdom */

import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type { CatalogPage } from "../../shared/contract.generated";
import { invoke } from "../../shared/ipc";
import type { MediaContextRequest } from "../library/media-actions";
import { CatalogView } from "./CatalogView";

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });

vi.mock("../../shared/ipc", () => ({
  invoke: vi.fn(),
  messageOf: (reason: unknown) => String(reason),
}));

class IntersectionObserverStub implements IntersectionObserver {
  static observedElements: Element[] = [];

  readonly root = null;
  readonly rootMargin = "0px";
  readonly scrollMargin = "0px";
  readonly thresholds = [0];

  disconnect() {}

  observe(target: Element) {
    IntersectionObserverStub.observedElements.push(target);
  }

  takeRecords(): IntersectionObserverEntry[] {
    return [];
  }

  unobserve() {}
}

describe("CatalogView", () => {
  beforeEach(() => {
    IntersectionObserverStub.observedElements = [];
    vi.stubGlobal("IntersectionObserver", IntersectionObserverStub);
    vi.mocked(invoke).mockResolvedValue({
      items: [
        {
          id: "movie-1",
          kind: "movie",
          title: "Movie 1",
          synopsis: "",
          genres: [],
          torrents: [],
        },
      ],
      page: 1,
      has_more: true,
    } satisfies CatalogPage);
  });

  afterEach(() => {
    vi.unstubAllGlobals();
    vi.clearAllMocks();
  });

  it("observes the paging anchor after the initial page finishes loading", async () => {
    const container = document.createElement("div");
    document.body.append(container);
    const root = createRoot(container);

    await act(async () => {
      root.render(
        <CatalogView
          initialKind="movie"
          onError={vi.fn()}
          onOpen={vi.fn()}
          onContext={vi.fn()}
          watchedMovies={[]}
          hideWatchedMovies
        />,
      );
      await Promise.resolve();
    });

    expect(IntersectionObserverStub.observedElements).toHaveLength(1);
    expect(
      IntersectionObserverStub.observedElements[0]?.classList.contains(
        "catalog-load-anchor",
      ),
    ).toBe(true);

    act(() => root.unmount());
    container.remove();
  });

  it("opens the shared media actions from a card context click", async () => {
    const container = document.createElement("div");
    document.body.append(container);
    const root = createRoot(container);
    const onContext = vi.fn<(request: MediaContextRequest) => void>();

    await act(async () => {
      root.render(
        <CatalogView
          initialKind="movie"
          onError={vi.fn()}
          onOpen={vi.fn()}
          onContext={onContext}
          watchedMovies={[]}
          hideWatchedMovies
        />,
      );
      await Promise.resolve();
    });

    act(() => {
      container.querySelector(".media-card")?.dispatchEvent(new MouseEvent("contextmenu", {
        bubbles: true,
        cancelable: true,
        clientX: 42,
        clientY: 84,
      }));
    });

    expect(onContext.mock.calls[0]?.[0]).toMatchObject({
      item: { id: "movie-1", kind: "movie" },
      x: 42,
      y: 84,
      continuation: false,
    });

    act(() => root.unmount());
    container.remove();
  });

  it("filters watched movies from loaded catalog results", async () => {
    vi.mocked(invoke).mockResolvedValueOnce({
      items: [{
        id: "movie-1",
        kind: "movie",
        title: "Movie 1",
        synopsis: "",
        genres: [],
        torrents: [],
      }],
      page: 1,
      has_more: false,
    } satisfies CatalogPage);
    const container = document.createElement("div");
    document.body.append(container);
    const root = createRoot(container);

    await act(async () => {
      root.render(
        <CatalogView
          initialKind="movie"
          onError={vi.fn()}
          onOpen={vi.fn()}
          onContext={vi.fn()}
          watchedMovies={[{ external_id: "MOVIE-1", kind: "movie" }]}
          hideWatchedMovies
        />,
      );
      await Promise.resolve();
    });

    expect(container.querySelector(".media-card")).toBeNull();
    expect(container.textContent).toContain("All loaded movies are watched");

    act(() => root.unmount());
    container.remove();
  });
});

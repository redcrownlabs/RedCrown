/** @vitest-environment jsdom */

import { act, useState } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type { CatalogPage, LibraryItem, MediaItem } from "../../shared/contract.generated";
import { invoke } from "../../shared/ipc";
import type { MediaContextRequest } from "../library/media-actions";
import { CatalogView } from "./CatalogView";
import { createCatalogSession } from "./catalog-session";

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

const noop = () => undefined;

function CatalogHarness({
  onOpen = noop,
  onContext = noop,
  watchedMovies = [],
}: {
  onOpen?: (item: MediaItem) => void;
  onContext?: (request: MediaContextRequest) => void;
  watchedMovies?: LibraryItem[];
}) {
  const [session, setSession] = useState(() => createCatalogSession("movie"));
  return (
    <CatalogView
      session={session}
      onSessionChange={setSession}
      onError={noop}
      onOpen={onOpen}
      onContext={onContext}
      watchedMovies={watchedMovies}
      hideWatchedMovies
    />
  );
}

function CatalogDrillDownHarness() {
  const [session, setSession] = useState(() => createCatalogSession("movie"));
  const [details, setDetails] = useState(false);
  if (details) {
    return <button onClick={() => setDetails(false)}>Back to catalog</button>;
  }
  return (
    <CatalogView
      session={session}
      onSessionChange={setSession}
      onError={noop}
      onOpen={() => setDetails(true)}
      onContext={noop}
      watchedMovies={[]}
      hideWatchedMovies
    />
  );
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
        <CatalogHarness />,
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
        <CatalogHarness onContext={onContext} />,
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
        <CatalogHarness watchedMovies={[{ external_id: "MOVIE-1", kind: "movie" }]} />,
      );
      await Promise.resolve();
    });

    expect(container.querySelector(".media-card")).toBeNull();
    expect(container.textContent).toContain("All loaded movies are watched");

    act(() => root.unmount());
    container.remove();
  });

  it("restores search, filters, sort, and loaded results after a details drill-down", async () => {
    const container = document.createElement("div");
    document.body.append(container);
    const root = createRoot(container);

    await act(async () => {
      root.render(<CatalogDrillDownHarness />);
      await Promise.resolve();
    });

    const search = container.querySelector<HTMLInputElement>('input[type="search"]');
    const sort = container.querySelector<HTMLSelectElement>('select[aria-label="Sort catalog"]');
    const genre = container.querySelector<HTMLSelectElement>('select[aria-label="Filter by genre"]');
    await act(async () => {
      if (!search || !sort || !genre) throw new Error("catalog controls missing");
      const inputValueDescriptor = Object.getOwnPropertyDescriptor(
        HTMLInputElement.prototype,
        "value",
      );
      if (!inputValueDescriptor?.set) throw new Error("input value setter missing");
      inputValueDescriptor.set.call(search, "silo");
      search.dispatchEvent(new Event("input", { bubbles: true }));
      sort.value = "rating";
      sort.dispatchEvent(new Event("change", { bubbles: true }));
      genre.value = "drama";
      genre.dispatchEvent(new Event("change", { bubbles: true }));
      await new Promise((resolve) => setTimeout(resolve, 0));
    });

    const requestsBeforeDrillDown = vi.mocked(invoke).mock.calls.length;
    act(() => container.querySelector<HTMLButtonElement>(".media-card")?.click());
    expect(container.textContent).toContain("Back to catalog");
    await act(async () => {
      container.querySelector<HTMLButtonElement>("button")?.click();
      await Promise.resolve();
    });

    expect(container.querySelector<HTMLInputElement>('input[type="search"]')?.value).toBe("silo");
    expect(container.querySelector<HTMLSelectElement>('select[aria-label="Sort catalog"]')?.value).toBe("rating");
    expect(container.querySelector<HTMLSelectElement>('select[aria-label="Filter by genre"]')?.value).toBe("drama");
    expect(container.querySelector(".media-card")).not.toBeNull();
    expect(vi.mocked(invoke)).toHaveBeenCalledTimes(requestsBeforeDrillDown);

    act(() => root.unmount());
    container.remove();
  });
});

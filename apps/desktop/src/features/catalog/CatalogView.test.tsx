/** @vitest-environment jsdom */

import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type { CatalogPage } from "../../shared/contract.generated";
import { invoke } from "../../shared/ipc";
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
});

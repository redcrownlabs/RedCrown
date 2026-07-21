/** @vitest-environment jsdom */

import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, describe, expect, it, vi } from "vitest";

import { ContextActionPopover } from "./ContextActionPopover";

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });

describe("ContextActionPopover", () => {
  afterEach(() => vi.restoreAllMocks());

  it("focuses an action and closes after it completes", async () => {
    Object.defineProperty(HTMLElement.prototype, "showPopover", {
      configurable: true,
      value: vi.fn(),
    });
    const container = document.createElement("div");
    document.body.append(container);
    const root = createRoot(container);
    const onSelect = vi.fn();
    const onClose = vi.fn();

    act(() => {
      root.render(
        <ContextActionPopover
          title="Silo"
          x={50}
          y={75}
          actions={[{ id: "watched", label: "Mark series as watched", onSelect }]}
          onClose={onClose}
        />,
      );
    });

    const button = container.querySelector("button");
    expect(document.activeElement).toBe(button);
    await act(async () => {
      button?.click();
      await Promise.resolve();
    });
    expect(onSelect).toHaveBeenCalledOnce();
    expect(onClose).toHaveBeenCalledOnce();

    act(() => root.unmount());
    container.remove();
    Reflect.deleteProperty(HTMLElement.prototype, "showPopover");
  });
});

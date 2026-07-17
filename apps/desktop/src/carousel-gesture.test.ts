import { describe, expect, it } from "vitest";

import { isCarouselDrag } from "./carousel-gesture";

describe("isCarouselDrag", () => {
  it("keeps click-sized movement below the drag threshold", () => {
    expect(isCarouselDrag(0)).toBe(false);
    expect(isCarouselDrag(6)).toBe(false);
    expect(isCarouselDrag(-6)).toBe(false);
  });

  it("activates dragging after meaningful horizontal movement", () => {
    expect(isCarouselDrag(7)).toBe(true);
    expect(isCarouselDrag(-7)).toBe(true);
  });
});

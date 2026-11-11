import { describe, expect, it } from "vitest";
import { excessOf, MOST_SCROLLED, nextAnchor, placeOf, windowIn, windowOf, type Anchor, type Layout } from "./virtual";

const grid: Layout = { columns: 4, rowStride: 94, columnStride: 104, itemWidth: 100, itemHeight: 90, top: 12, left: 12, gap: 4, marginTop: 12, marginBottom: 12 };

describe("windowOf", () => {
  it("stops at the last item", () => {
    const w = windowOf(grid, 10, 0, 600, 3);
    expect(w).toEqual({ start: 0, end: 10, before: 0, after: 0 });
  });

  it("positions rendered rows", () => {
    const w = windowOf(grid, 1000, 5000, 600, 3);
    expect(w.before + grid.gap + grid.top).toBe(placeOf(grid, w.start).top);
  });
});

const list: Layout = { columns: 1, rowStride: 36, columnStride: 0, itemWidth: 600, itemHeight: 36, top: 36, left: 0, gap: 0, marginTop: 36, marginBottom: 0 };
const MILLION = 1_000_000;
const HEIGHT = 900;

describe("very tall views", () => {
  function scrollThrough(from: Anchor, step: number) {
    const excess = excessOf(list, MILLION);
    const most = list.top + MOST_SCROLLED - HEIGHT;
    let anchor = from;
    let lastTop = from.scrollTop;
    let anchoredAgain = 0;
    for (let n = 0; n < 1_000_000; n++) {
      const scrollTop = Math.max(0, Math.min(most, anchor.scrollTop + step));
      if (scrollTop === anchor.scrollTop) break;
      const shownBefore = anchor.scrollTop + anchor.offset;
      const next = nextAnchor(list, MILLION, HEIGHT, { offset: anchor.offset, lastTop, scrollTop: anchor.scrollTop }, { scrollTop });
      expect(next.scrollTop + next.offset).toBeCloseTo(shownBefore + (scrollTop - anchor.scrollTop), 6);
      expect(next.offset).toBeGreaterThanOrEqual(0);
      expect(next.offset).toBeLessThanOrEqual(excess);
      expect(next.scrollTop).toBeGreaterThanOrEqual(0);
      expect(windowIn(list, MILLION, next, HEIGHT, 3, excess)).not.toBeNull();
      if (next.scrollTop !== scrollTop) anchoredAgain++;
      anchor = next;
      lastTop = next.scrollTop;
    }
    return { anchor, anchoredAgain, window: windowIn(list, MILLION, anchor, HEIGHT, 3, excess)! };
  }

  it("scrolls smoothly through a very tall list", () => {
    const down = scrollThrough({ scrollTop: 0, offset: 0 }, 997);
    expect(down.window.end).toBe(MILLION);
    expect(down.anchor.offset).toBe(excessOf(list, MILLION));
    expect(down.anchoredAgain).toBeGreaterThan(0);
    expect(down.anchoredAgain).toBeLessThan(10);
    const up = scrollThrough(down.anchor, -997);
    expect(up.window.start).toBe(0);
    expect(up.anchor).toEqual({ scrollTop: 0, offset: 0 });
    expect(up.anchoredAgain).toBeGreaterThan(0);
    expect(up.anchoredAgain).toBeLessThan(10);
  });
});

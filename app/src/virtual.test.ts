import { describe, expect, it } from "vitest";
import { placeOf, windowOf, type Layout } from "./virtual";

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

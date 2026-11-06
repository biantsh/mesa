import { describe, expect, it } from "vitest";
import { touching, type Placed } from "./marquee";

const at = (path: string, left: number, top: number, size = 10): Placed => ({
  path,
  left,
  top,
  right: left + size,
  bottom: top + size,
});

describe("touching", () => {
  const grid = [at("a", 0, 0), at("b", 20, 0), at("c", 0, 20), at("d", 20, 20)];

  it("selects items it touches", () => {
    expect(touching(grid, { left: 8, top: 8, right: 22, bottom: 22 })).toEqual(["a", "b", "c", "d"]);
    expect(touching(grid, { left: 12, top: 0, right: 25, bottom: 5 })).toEqual(["b"]);
    expect(touching(grid, { left: 12, top: 12, right: 18, bottom: 18 })).toEqual([]);
  });
});

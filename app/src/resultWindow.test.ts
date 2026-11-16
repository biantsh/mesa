import { describe, expect, it } from "vitest";
import { moveAmong, resultWindow, type Layout } from "./resultWindow";

const layout: Layout = { columns: 6, stride: 100, divider: 40, gap: 4 };

describe("resultWindow", () => {
  it("is clamped to the number of results", () => {
    const made = resultWindow(30, null, layout, 50_000, 50_400, 2);
    expect(made.end).toBe(30);
    expect(made.after).toBe(0);
  });
});

describe("moveAmong", () => {
  it("crosses the divider in the same column", () => {
    expect(moveAmong(2, "ArrowDown", 18, 8, 6)).toBe(7);
    expect(moveAmong(7, "ArrowDown", 18, 8, 6)).toBe(9);
    expect(moveAmong(6, "ArrowDown", 18, 8, 6)).toBe(8);
    expect(moveAmong(10, "ArrowUp", 18, 8, 6)).toBe(7);
    expect(moveAmong(8, "ArrowUp", 18, 8, 6)).toBe(6);
    expect(moveAmong(15, "ArrowUp", 18, 8, 6)).toBe(9);
  });
});

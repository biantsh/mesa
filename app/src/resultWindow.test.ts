import { describe, expect, it } from "vitest";
import { resultWindow, type Layout } from "./resultWindow";

const layout: Layout = { columns: 6, stride: 100, divider: 40, gap: 4 };

describe("resultWindow", () => {
  it("is clamped to the number of results", () => {
    const made = resultWindow(30, null, layout, 50_000, 50_400, 2);
    expect(made.end).toBe(30);
    expect(made.after).toBe(0);
  });
});

import { describe, expect, it } from "vitest";
import { isWheelTurn } from "./smoothWheel";

describe("isWheelTurn", () => {
  it("handles mouse wheel notches", () => {
    expect(isWheelTurn(0, -120, Infinity)).toBe(true);
    expect(isWheelTurn(0, 240, Infinity)).toBe(true);
  });

  it("ignores trackpad scrolling", () => {
    expect(isWheelTurn(0, -37, Infinity)).toBe(false);
    expect(isWheelTurn(0, 4.5, Infinity)).toBe(false);
    expect(isWheelTurn(0, undefined, Infinity)).toBe(false);
    expect(isWheelTurn(0, 0, Infinity)).toBe(false);
  });
});

import { describe, expect, it } from "vitest";
import { WINDOW_MS, addSample, formatEta, speedOf, type Sample } from "./eta";

const at = (seconds: number, done: number): Sample => ({ t: seconds * 1000, done });

describe("speedOf", () => {
  it("measures files per second", () => {
    expect(speedOf([at(0, 0), at(10, 25)])).toBeCloseTo(2.5);
    expect(speedOf([at(0, 0), at(2, 25)])).toBeNull();
    expect(speedOf([at(0, 5)])).toBeNull();
    expect(speedOf([])).toBeNull();
  });
});

describe("addSample", () => {
  it("resets when the done count goes down", () => {
    const samples = addSample(addSample([], at(0, 50)), at(5, 60));
    expect(addSample(samples, at(6, 3))).toEqual([at(6, 3)]);
  });

  it("drops samples outside the window", () => {
    let samples: Sample[] = [];
    for (let s = 0; s <= 100; s += 10) samples = addSample(samples, at(s, s));
    const oldest = samples[0].t;
    expect(oldest).toBeGreaterThanOrEqual(100_000 - WINDOW_MS - 10_000);
    expect(oldest).toBeLessThanOrEqual(100_000 - WINDOW_MS);
    expect(samples[samples.length - 1]).toEqual(at(100, 100));
  });
});

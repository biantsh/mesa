import { describe, expect, it } from "vitest";
import * as events from "./events";

const rustFiles = import.meta.glob("../src-tauri/src/*.rs", { query: "?raw", import: "default", eager: true }) as Record<string, string>;
const rust = Object.values(rustFiles).join("\n");

describe("event names", () => {
  it("match the Rust source", () => {
    const names = Object.values(events);
    expect(Object.keys(rustFiles).length).toBeGreaterThan(5);
    expect(names.length).toBeGreaterThan(0);
    for (const name of names) expect(rust, `"${name}" is not an event name in the Rust source`).toContain(`"${name}"`);
  });

  it("are unique", () => {
    const names = Object.values(events);
    expect(new Set(names).size).toBe(names.length);
  });
});

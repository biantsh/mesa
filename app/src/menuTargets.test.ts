import { describe, expect, it } from "vitest";
import type { Entry, IndexStatus } from "./api";
import { menuTargets } from "./menuTargets";

const entry = (name: string, index: IndexStatus, more: Partial<Entry> = {}): Entry => ({
  name,
  path: `/home/${name}`,
  kind: "folder",
  category: null,
  readable: true,
  hasContents: true,
  index,
  queued: false,
  excludedHere: false,
  removable: false,
  size: null,
  modified: null,
  ...more,
});

const names = (entries: Entry[]) => entries.map((e) => e.name);

describe("menuTargets", () => {
  it("only allows excluding items inside an indexed folder", () => {
    const inside = entry("inside", "indexed", { removable: false });
    const { remove, exclude, inside: only } = menuTargets([inside]);
    expect(remove).toEqual([]);
    expect(names(exclude)).toEqual(["inside"]);
    expect(names(only)).toEqual(["inside"]);
  });

  it("handles a mixed selection", () => {
    const targets = [
      entry("added", "indexed", { removable: true }),
      entry("none", "notIndexed"),
      entry("excluded", "excluded", { excludedHere: true }),
    ];
    const { remove, add, exclude, stop } = menuTargets(targets);
    expect(names(remove)).toEqual(["added"]);
    expect(names(add)).toEqual(["none"]);
    expect(names(exclude)).toEqual(["added", "none"]);
    expect(names(stop)).toEqual(["excluded"]);
  });
});

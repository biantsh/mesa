import { describe, expect, it } from "vitest";
import type { Entry } from "./api";
import { applyRuns, entriesOf, listedOf, withStatusChanges, type Columns } from "./listingColumns";

const entry = (fields: Partial<Entry>): Entry => ({
  name: "",
  path: "",
  kind: "file",
  category: null,
  readable: true,
  hasContents: false,
  index: "notIndexed",
  queued: false,
  excludedHere: false,
  removable: false,
  size: null,
  modified: 1,
  privacy: null,
  ...fields,
});

describe("applyRuns", () => {
  const was = ["a", "b", "c", "d"].map((name) => entry({ name, path: `/x/${name}` }));

  it("reuses unchanged entries", () => {
    const fresh = [entry({ name: "bb", path: "/x/bb" })];
    const made = applyRuns(was, [[0, 2], 1, [3, 1]], fresh)!;
    expect(made.map((it) => it.name)).toEqual(["a", "b", "bb", "d"]);
    expect(made[0]).toBe(was[0]);
    expect(made[2]).toBe(fresh[0]);
  });

  it("detects mismatched runs", () => {
    expect(applyRuns(was, [[2, 5]], [])).toBeNull();
    expect(applyRuns(was, [2], [entry({})])).toBeNull();
    expect(applyRuns(was, [[0, 1]], [entry({})])).toBeNull();
  });
});

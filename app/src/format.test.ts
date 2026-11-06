import { describe, expect, it } from "vitest";
import { nameParts } from "./format";

describe("nameParts", () => {
  it("splits camelCase", () => {
    expect(nameParts("dotTraceSnapshots")).toEqual(["dot", "Trace", "Snapshots"]);
    expect(nameParts("Lab1Project")).toEqual(["Lab1", "Project"]);
  });
});

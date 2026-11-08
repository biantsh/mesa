import { describe, expect, it } from "vitest";
import { MAX_EXAMPLES, addExamples, categoryOfPath, countByKind, distinctNames, dropHint, dropIcon, fileNameOf, shortName } from "./examples";

describe("distinctNames", () => {
  it("adds the folder only to duplicate names", () => {
    expect(distinctNames(["C:\\a\\x.jpg", "C:\\b\\y.jpg"])).toEqual(["x.jpg", "y.jpg"]);
    expect(distinctNames(["/me/a/x.jpg", "/me/b/x.jpg"])).toEqual(["x.jpg (a)", "x.jpg (b)"]);
  });
});

describe("shortName", () => {
  it("truncates the middle and keeps the extension", () => {
    const cut = shortName("Joe_Rogan_Admits_Hes_SCARED_of_Mike_Tyson.mp3");
    expect(cut).toHaveLength(34);
    expect(cut.startsWith("Joe_Rogan")).toBe(true);
    expect(cut.endsWith("….mp3")).toBe(true);
  });
});

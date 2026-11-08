import { describe, expect, it } from "vitest";
import { fitQuoted } from "./fitText";

const measure = (text: string) => text.length * 10;

describe("fitQuoted", () => {
  it("truncates with an ellipsis inside the quotes", () => {
    expect(fitQuoted("dog walking on a beach", 90, measure)).toBe("\u201cdog wa\u2026\u201d");
  });

  it("does not split surrogate pairs", () => {
    expect(fitQuoted("\u{1F415}\u{1F415}\u{1F415}\u{1F415}", 40, (s) => Array.from(s).length * 10)).toBe("\u201c\u{1F415}\u2026\u201d");
  });
});

import { describe, expect, it } from "vitest";
import { middleCuts } from "./pathCut";

const joined = (cuts: string[][]) => cuts.map((parts) => parts.join(""));

describe("middleCuts", () => {
  it("shortens the middle of a long path", () => {
    expect(joined(middleCuts(String.raw`C:\Users\shkol\Downloads\mesa-windows-test\testdata\commons\devanagari`))).toEqual([
      String.raw`C:\Users\shkol\Downloads\mesa-windows-test\testdata\commons\devanagari`,
      String.raw`C:\Users\shkol\Downloads\…\testdata\commons\devanagari`,
      String.raw`C:\Users\shkol\Downloads\…\commons\devanagari`,
      String.raw`C:\Users\shkol\Downloads\…\devanagari`,
    ]);
  });
});

import { describe, expect, it } from "vitest";
import { navigate } from "./navigation";
import { pick, type Picked } from "./selection";

const all = (n: number) => Array<boolean>(n).fill(true);

describe("navigate", () => {
  it("moves by row with up and down", () => {
    expect(navigate(all(10), 1, "ArrowDown", 4)).toBe(5);
    expect(navigate(all(10), 5, "ArrowUp", 4)).toBe(1);
    expect(navigate(all(10), 1, "ArrowUp", 4)).toBeNull();
  });

  it("skips disabled items", () => {
    const selectable = [true, false, true, false, false, true];
    expect(navigate(selectable, 0, "ArrowRight", 3)).toBe(2);
    expect(navigate(selectable, 5, "ArrowLeft", 3)).toBe(2);
    expect(navigate(selectable, 2, "ArrowDown", 1)).toBe(5);
    expect(navigate(selectable, 0, "ArrowDown", 3)).toBe(5);
  });
});

describe("pick", () => {
  const order = ["a", "b", "c", "d", "e"];
  const from = (paths: string[], anchor: string, active: string): Picked => ({ paths: new Set(paths), anchor, active });

  it("selects a range from the anchor", () => {
    const down = pick(from(["b"], "b", "b"), order, "d", "range");
    expect([...down.paths]).toEqual(["b", "c", "d"]);
    expect([down.anchor, down.active]).toEqual(["b", "d"]);

    const up = pick(down, order, "a", "range");
    expect([...up.paths]).toEqual(["a", "b"]);
    expect(up.anchor).toBe("b");
  });
});

import { describe, expect, it } from "vitest";
import { NONE_HELD, anyHeld, heldFrom, isKeyCode, isModifierCode, keyLabel, modifierLabels, shortcutString, type Held } from "./hotkey";

const held = (...on: (keyof Held)[]): Held => ({ ...NONE_HELD, ...Object.fromEntries(on.map((modifier) => [modifier, true])) });

describe("keys", () => {
  it("separates modifiers from the key", () => {
    for (const code of ["ControlLeft", "ControlRight", "AltLeft", "ShiftRight", "MetaLeft"]) {
      expect(isModifierCode(code)).toBe(true);
      expect(isKeyCode(code)).toBe(false);
    }
    for (const code of ["KeyK", "Space", "F5", "Digit1", "ArrowUp"]) expect(isKeyCode(code)).toBe(true);
  });
});

describe("keycaps", () => {
  it("orders modifiers like Rust", () => {
    const all = held("meta", "shift", "alt", "ctrl");
    expect(modifierLabels(all, "windows")).toEqual(["Ctrl", "Alt", "Shift", "Win"]);
    expect(modifierLabels(all, "linux")).toEqual(["Ctrl", "Alt", "Shift", "Super"]);
    expect(modifierLabels(all, "mac")).toEqual(["⌃", "⌥", "⇧", "⌘"]);
    expect(modifierLabels(NONE_HELD, "mac")).toEqual([]);
  });
});

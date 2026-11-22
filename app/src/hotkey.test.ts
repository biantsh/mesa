import { describe, expect, it } from "vitest";
import { NONE_HELD, anyHeld, heldAfter, heldFrom, heldKeys, isKeyCode, isModifierCode, keyLabel, modifierLabels, shortcutString, type Held } from "./hotkey";

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

describe("held keys", () => {
  const event = (type: string, code: string, flags: Partial<Record<"ctrlKey" | "altKey" | "shiftKey" | "metaKey", boolean>>) => ({
    type,
    code,
    ctrlKey: false,
    altKey: false,
    shiftKey: false,
    metaKey: false,
    ...flags,
  });

  it("lets a modifier go on its own key-up, even when the event still says it's held", () => {
    expect(heldAfter(event("keyup", "AltLeft", { altKey: true }))).toEqual(NONE_HELD);
    expect(heldAfter(event("keyup", "ControlRight", { ctrlKey: true, altKey: true }))).toEqual({ ...NONE_HELD, alt: true });
  });

  it("believes Super only from its own keys, not from Alt typed as Meta after Shift", () => {
    const held = heldKeys();
    expect(held(event("keydown", "ShiftLeft", { shiftKey: true }))).toEqual({ ...NONE_HELD, shift: true });
    expect(held(event("keydown", "AltLeft", { shiftKey: true, metaKey: true }))).toEqual({ ...NONE_HELD, shift: true, alt: true });
    expect(held(event("keydown", "MetaLeft", { metaKey: true }))).toEqual({ ...NONE_HELD, meta: true });
    expect(held(event("keydown", "KeyE", { metaKey: true }))).toEqual({ ...NONE_HELD, meta: true });
    expect(held(event("keyup", "MetaLeft", { metaKey: true }))).toEqual(NONE_HELD);
  });

  it("holds a modifier on its own key-down, and leaves other keys to the event", () => {
    expect(heldAfter(event("keydown", "ShiftLeft", {}))).toEqual({ ...NONE_HELD, shift: true });
    expect(heldAfter(event("keyup", "KeyE", { altKey: true }))).toEqual({ ...NONE_HELD, alt: true });
  });
});

import type { Platform } from "./platform";

export interface Held {
  ctrl: boolean;
  alt: boolean;
  shift: boolean;
  meta: boolean;
}

export const NONE_HELD: Held = { ctrl: false, alt: false, shift: false, meta: false };

const MODIFIER_CODES = new Set(["ControlLeft", "ControlRight", "AltLeft", "AltRight", "ShiftLeft", "ShiftRight", "MetaLeft", "MetaRight", "OS"]);

export const isModifierCode = (code: string) => MODIFIER_CODES.has(code);

export const isKeyCode = (code: string) => code !== "" && code !== "Unidentified" && !isModifierCode(code);

export function heldFrom(event: Pick<KeyboardEvent, "ctrlKey" | "altKey" | "shiftKey" | "metaKey">): Held {
  return { ctrl: event.ctrlKey, alt: event.altKey, shift: event.shiftKey, meta: event.metaKey };
}

const MODIFIER_OF: Record<string, keyof Held> = {
  ControlLeft: "ctrl",
  ControlRight: "ctrl",
  AltLeft: "alt",
  AltRight: "alt",
  ShiftLeft: "shift",
  ShiftRight: "shift",
  MetaLeft: "meta",
  MetaRight: "meta",
  OS: "meta",
};

// What's held after this key event. WebKitGTK reports a modifier's own key-up with it still held (GTK gives the state
// from before the event), so that modifier is taken from the event's type.
export function heldAfter(event: Pick<KeyboardEvent, "type" | "code" | "ctrlKey" | "altKey" | "shiftKey" | "metaKey">): Held {
  const held = heldFrom(event);
  const modifier = MODIFIER_OF[event.code];
  if (modifier) held[modifier] = event.type === "keydown";
  return held;
}

// Tracks what's held across key events, trusting Super only from its own keys: in the usual Linux layouts, Alt pressed
// after Shift types Meta, which WebKitGTK reports as metaKey.
export function heldKeys() {
  const supers = new Set<string>();
  return (event: Pick<KeyboardEvent, "type" | "code" | "ctrlKey" | "altKey" | "shiftKey" | "metaKey">): Held => {
    if (MODIFIER_OF[event.code] === "meta") {
      if (event.type === "keydown") supers.add(event.code);
      else supers.delete(event.code);
    }
    return { ...heldAfter(event), meta: supers.size > 0 };
  };
}

export const anyHeld = (held: Held) => held.ctrl || held.alt || held.shift || held.meta;

export function shortcutString(held: Held, code: string): string {
  const modifiers = [held.ctrl && "control", held.alt && "alt", held.shift && "shift", held.meta && "super"].filter(Boolean);
  return [...modifiers, code].join("+");
}

const SYMBOLS = { ctrl: "⌃", alt: "⌥", shift: "⇧", meta: "⌘" } as const;
const WORDS = { ctrl: "Ctrl", alt: "Alt", shift: "Shift", meta: "Win" } as const;

export function modifierLabels(held: Held, platform: Platform): string[] {
  const names = platform === "mac" ? SYMBOLS : platform === "linux" ? { ...WORDS, meta: "Super" } : WORDS;
  return (["ctrl", "alt", "shift", "meta"] as const).filter((modifier) => held[modifier]).map((modifier) => names[modifier]);
}

const NAMED: Record<string, string> = {
  ArrowUp: "↑",
  ArrowDown: "↓",
  ArrowLeft: "←",
  ArrowRight: "→",
  PageUp: "PgUp",
  PageDown: "PgDn",
  Backquote: "`",
  Minus: "-",
  Equal: "=",
  BracketLeft: "[",
  BracketRight: "]",
  Backslash: "\\",
  Semicolon: ";",
  Quote: "'",
  Comma: ",",
  Period: ".",
  Slash: "/",
};

export function keyLabel(code: string): string {
  return NAMED[code] ?? code.replace(/^(Key|Digit)/, "");
}

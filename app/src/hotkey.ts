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

export const anyHeld = (held: Held) => held.ctrl || held.alt || held.shift || held.meta;

export function shortcutString(held: Held, code: string): string {
  const modifiers = [held.ctrl && "control", held.alt && "alt", held.shift && "shift", held.meta && "super"].filter(Boolean);
  return [...modifiers, code].join("+");
}

const SYMBOLS = { ctrl: "⌃", alt: "⌥", shift: "⇧", meta: "⌘" } as const;
const WORDS = { ctrl: "Ctrl", alt: "Alt", shift: "Shift", meta: "Win" } as const;

export function modifierLabels(held: Held, mac: boolean): string[] {
  const names = mac ? SYMBOLS : WORDS;
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

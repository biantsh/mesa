export const isMac = typeof navigator !== "undefined" && /Mac/i.test(navigator.platform || navigator.userAgent);

export const fileManager = isMac ? "Finder" : "File Explorer";

export function shortcut(key: string, shift = false): string {
  if (isMac) return `${shift ? "⇧" : ""}⌘${key}`;
  return `Ctrl+${shift ? "Shift+" : ""}${key}`;
}

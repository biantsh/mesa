export type Platform = "mac" | "windows" | "linux";

export const platform: Platform = (() => {
  const name = typeof navigator === "undefined" ? "" : navigator.platform || navigator.userAgent;
  if (/Mac/i.test(name)) return "mac";
  if (/Linux/i.test(name)) return "linux";
  return "windows";
})();

export const isMac = platform === "mac";
export const isWebKit = platform !== "windows";

export const fileManager = { mac: "Finder", windows: "File Explorer", linux: "file manager" }[platform];

export function shortcut(key: string, shift = false): string {
  if (isMac) return `${shift ? "⇧" : ""}⌘${key}`;
  return `Ctrl+${shift ? "Shift+" : ""}${key}`;
}

export function markPlatform() {
  if (platform !== "windows") document.documentElement.dataset.platform = platform;
}

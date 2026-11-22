export type Platform = "mac" | "windows" | "linux";

export const platform: Platform = (() => {
  const name = typeof navigator === "undefined" ? "" : navigator.platform || navigator.userAgent;
  if (/Mac/i.test(name)) return "mac";
  if (/Linux/i.test(name)) return "linux";
  return "windows";
})();

export const isMac = platform === "mac";
export const isWebKit = platform !== "windows";

// Tauri renders with WebKitGTK on Linux, which stutters starting and ending GPU animations of large surfaces, so
// there those run on the main thread or not at all (see animateLarge and the data-engine rules).
export const isWebKitGTK = (() => {
  const agent = typeof navigator === "undefined" ? "" : navigator.userAgent;
  return /Linux/.test(agent) && /AppleWebKit/.test(agent) && !/Chrome|Chromium|Edg\//.test(agent);
})();

export const fileManager = { mac: "Finder", windows: "File Explorer", linux: "file manager" }[platform];

export function shortcut(key: string, shift = false): string {
  if (isMac) return `${shift ? "⇧" : ""}⌘${key}`;
  return `Ctrl+${shift ? "Shift+" : ""}${key}`;
}

export function markPlatform() {
  if (platform !== "windows") document.documentElement.dataset.platform = platform;
}

export function markEngine() {
  if (isWebKitGTK) document.documentElement.dataset.engine = "webkitgtk";
}

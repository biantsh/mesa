export type Window = never;

export function getCurrentWindow(): Window {
  throw new Error("No Tauri window: the site's live demo.");
}

import { useEffect, useRef } from "react";
import { api, type Entry, type Listing } from "./api";
import { FOLDER_CHANGED } from "./events";
import { useTauriEvent } from "./useTauriEvent";

export function useFolderChanges(folder: string | null, active: boolean, refresh: () => void) {
  const watching = active ? folder : null;
  useEffect(() => {
    api.watchShownFolder(watching).catch(() => {});
  }, [watching]);
  useEffect(() => () => void api.watchShownFolder(null).catch(() => {}), []);

  const wasActive = useRef(active);
  useEffect(() => {
    if (active && !wasActive.current && folder !== null) refresh();
    wasActive.current = active;
  }, [active, folder, refresh]);

  useTauriEvent<string>(FOLDER_CHANGED, (changed) => {
    if (active && changed === folder) refresh();
  });
}

export function mergeListing(shown: Listing, next: Listing): Listing {
  const before = new Map(shown.entries.map((entry) => [entry.path, entry]));
  let changed = next.entries.length !== shown.entries.length || next.path !== shown.path;
  const entries = next.entries.map((entry, i) => {
    const old = before.get(entry.path);
    if (old && sameEntry(old, entry)) {
      if (shown.entries[i] !== old) changed = true;
      return old;
    }
    changed = true;
    return entry;
  });
  return changed ? { ...next, entries } : shown;
}

function sameEntry(a: Entry, b: Entry): boolean {
  const keys = new Set([...Object.keys(a), ...Object.keys(b)]) as Set<keyof Entry>;
  for (const key of keys) if (a[key] !== b[key]) return false;
  return true;
}

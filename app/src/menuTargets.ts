import type { Entry } from "./api";

export interface MenuTargets {
  add: Entry[];
  remove: Entry[];
  inside: Entry[];
  exclude: Entry[];
  stop: Entry[];
}

export function menuTargets(entries: Entry[]): MenuTargets {
  return {
    add: entries.filter((entry) => entry.index === "notIndexed" || entry.index === "partlyIndexed"),
    remove: entries.filter((entry) => entry.removable),
    inside: entries.filter((entry) => entry.index === "indexed" && !entry.removable),
    exclude: entries.filter(
      (entry) => entry.readable && entry.index !== "excluded" && entry.index !== "unindexable",
    ),
    stop: entries.filter((entry) => entry.excludedHere),
  };
}

const KIND = { drive: "drive", folder: "folder", file: "file" } as const;

export function describeKinds(entries: Entry[]): string {
  if (entries.length === 1) return KIND[entries[0].kind];
  const kinds = new Set(entries.map((entry) => entry.kind));
  return kinds.size === 1 ? `${entries.length} ${KIND[entries[0].kind]}s` : `${entries.length} items`;
}

export function excludeLabel(entries: Entry[], applicable: Entry[]): string {
  return `Exclude ${describeKinds(applicable.length > 0 ? applicable : entries)}`;
}

export function counted(label: string, count: number, forSeveral: boolean): string {
  return forSeveral && count > 0 ? `${label} (${count})` : label;
}

export function wholeFolderAdd(
  folderPath: string,
  entries: Entry[],
  targets: Entry[],
  add: Entry[],
): "folder" | "completing" | null {
  if (folderPath === "" || add.length === 0) return null;
  const chosen = new Set(targets.map((entry) => entry.path));
  const adding = new Set(add.map((entry) => entry.path));
  const everythingChosen = entries.every((entry) => !entry.readable || chosen.has(entry.path));
  const everythingAccountedFor = entries.every(
    (entry) =>
      !entry.readable ||
      entry.index === "unindexable" ||
      entry.index === "excluded" ||
      entry.index === "typeNotIndexed" ||
      adding.has(entry.path) ||
      entry.index === "indexed",
  );
  if (!everythingChosen || !everythingAccountedFor) return null;
  return entries.some((entry) => entry.index === "indexed") ? "completing" : "folder";
}

export const MANY_TO_ADD = 500;

export function whenAdding(
  folderPath: string,
  entries: Entry[],
  targets: Entry[],
  add: Entry[],
): { instead: "folder" | "completing"; everything: boolean } | null {
  if (folderPath === "" || add.length === 0) return null;
  const whole = wholeFolderAdd(folderPath, entries, targets, add);
  if (whole !== null) return { instead: whole, everything: true };
  return add.length >= MANY_TO_ADD ? { instead: "folder", everything: false } : null;
}

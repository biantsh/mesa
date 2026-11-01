import type { Entry } from "./api";

export interface MenuTargets {
  add: Entry[];
  drop: Entry[];
  remove: Entry[];
  exclude: Entry[];
  stop: Entry[];
}

export function menuTargets(entries: Entry[]): MenuTargets {
  return {
    add: entries.filter((entry) => entry.index === "notIndexed" || entry.index === "partlyIndexed"),
    drop: entries.filter((entry) => entry.droppable),
    remove: entries.filter((entry) => entry.index === "indexed" || entry.index === "partlyIndexed"),
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
      adding.has(entry.path) ||
      entry.index === "indexed",
  );
  if (!everythingChosen || !everythingAccountedFor) return null;
  return entries.some((entry) => entry.index === "indexed") ? "completing" : "folder";
}

import type { Entry } from "./api";
import { typeLabel } from "./format";

export type SortColumn = "name" | "status" | "type" | "size" | "modified";
export type SortDirection = "asc" | "desc";

export interface Sort {
  column: SortColumn;
  direction: SortDirection;
}

export const DEFAULT_SORT: Sort = { column: "name", direction: "asc" };

export function nextSort(current: Sort, column: SortColumn): Sort {
  if (current.column === column) {
    return { column, direction: current.direction === "asc" ? "desc" : "asc" };
  }
  return { column, direction: column === "size" || column === "modified" ? "desc" : "asc" };
}

const STATUS_ORDER = {
  indexed: 0,
  partlyIndexed: 1,
  indexing: 2,
  paused: 3,
  notIndexed: 4,
  typeNotIndexed: 5,
  excluded: 6,
  unindexable: 7,
} as const;

function statusRank(entry: Entry): number {
  return entry.readable ? STATUS_ORDER[entry.index] : 8;
}

function compareColumn(a: Entry, b: Entry, column: SortColumn): number | null {
  switch (column) {
    case "name":
      return 0;
    case "status":
      return statusRank(a) - statusRank(b);
    case "type": {
      const [x, y] = [typeLabel(a), typeLabel(b)];
      return x < y ? -1 : x > y ? 1 : 0;
    }
    case "size":
      return a.size === null || b.size === null ? null : a.size - b.size;
    case "modified":
      return a.modified === null || b.modified === null ? null : a.modified - b.modified;
  }
}

export function sortEntries(entries: Entry[], { column, direction }: Sort): Entry[] {
  const sign = direction === "asc" ? 1 : -1;
  const isFile = (entry: Entry) => entry.kind === "file";
  const position = new Map(entries.map((entry, i) => [entry.path, i]));
  const byName = (a: Entry, b: Entry) => (position.get(a.path) ?? 0) - (position.get(b.path) ?? 0);

  return [...entries].sort((a, b) => {
    if (isFile(a) !== isFile(b)) return isFile(a) ? 1 : -1;
    if (column === "name") return sign * byName(a, b);
    const difference = compareColumn(a, b, column);
    if (difference === null) {
      const [aMissing, bMissing] = [missing(a, column), missing(b, column)];
      if (aMissing !== bMissing) return aMissing ? 1 : -1;
      return byName(a, b);
    }
    return difference === 0 ? byName(a, b) : sign * difference;
  });
}

function missing(entry: Entry, column: SortColumn): boolean {
  return (column === "size" && entry.size === null) || (column === "modified" && entry.modified === null);
}

export function samePaths(a: Entry[], b: Entry[]): boolean {
  return a === b || (a.length === b.length && a.every((entry, i) => entry.path === b[i].path));
}

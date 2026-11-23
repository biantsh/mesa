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
  partlyCloudOnly: 1,
  indexing: 2,
  paused: 3,
  notIndexed: 4,
  cloudOnly: 5,
  typeNotIndexed: 6,
  excluded: 7,
  unindexable: 8,
} as const;

function statusRank(entry: Entry): number {
  return entry.readable ? STATUS_ORDER[entry.index] : 9;
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

export function sortEntries(entries: Entry[], sort: Sort): Entry[] {
  return sortedOrder(entries, sort).map((at) => entries[at]);
}

export function sortedOrder(entries: readonly Entry[], { column, direction }: Sort): number[] {
  const sign = direction === "asc" ? 1 : -1;
  const isFile = (entry: Entry) => entry.kind === "file";
  const order = entries.map((_, at) => at);
  return order.sort((i, j) => {
    const [a, b] = [entries[i], entries[j]];
    if (isFile(a) !== isFile(b)) return isFile(a) ? 1 : -1;
    if (column === "name") return sign * (i - j);
    const difference = compareColumn(a, b, column);
    if (difference === null) {
      const [aMissing, bMissing] = [missing(a, column), missing(b, column)];
      if (aMissing !== bMissing) return aMissing ? 1 : -1;
      return i - j;
    }
    return difference === 0 ? i - j : sign * difference;
  });
}

function missing(entry: Entry, column: SortColumn): boolean {
  return (column === "size" && entry.size === null) || (column === "modified" && entry.modified === null);
}

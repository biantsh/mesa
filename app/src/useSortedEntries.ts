import { useMemo, useRef } from "react";
import type { Entry } from "./api";
import { samePaths, sortEntries, type Sort } from "./sort";

export function useSortedEntries(entries: Entry[], sort: Sort): Entry[] {
  const worked = useRef<{ entries: Entry[]; sort: Sort; order: string[] } | null>(null);
  const last = worked.current;
  if (!last || last.sort !== sort || !samePaths(last.entries, entries)) {
    worked.current = { entries, sort, order: sortEntries(entries, sort).map((entry) => entry.path) };
  } else {
    last.entries = entries;
  }
  const order = worked.current!.order;

  return useMemo(() => {
    const byPath = new Map(entries.map((entry) => [entry.path, entry]));
    return order.flatMap((path) => byPath.get(path) ?? []);
  }, [order, entries]);
}

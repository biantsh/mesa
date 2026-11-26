import { useMemo, useRef } from "react";
import type { Entry } from "./api";
import { shapeOf, withShape, type Shape } from "./entriesShape";
import { sortedOrder, type Sort } from "./sort";

export function useSortedEntries(entries: Entry[], sort: Sort, counts: ReadonlyMap<string, number> | null): Entry[] {
  const worked = useRef<{ of: Shape; sort: Sort; counts: ReadonlyMap<string, number> | null; order: number[]; shape: Shape } | null>(null);
  const of = shapeOf(entries);
  const last = worked.current;
  if (!last || last.sort !== sort || last.of !== of || (sort.column === "size" && last.counts !== counts)) {
    const order = sortedOrder(entries, sort, counts);
    const asListed = order.every((at, i) => at === i);
    worked.current = { of, sort, counts, order, shape: { rust: of.rust, order: asListed ? null : order } };
  }
  const { order, shape } = worked.current!;

  return useMemo(() => withShape(order.map((at) => entries[at]), shape), [order, entries, shape]);
}

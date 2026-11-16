import { useCallback, useEffect, useMemo, useState } from "react";
import type { SearchHit } from "./api";
import { howToPick, NOTHING_PICKED, pick, type Pick, type Picked } from "./selection";
import { useLatest } from "./useLatest";

function keep(picked: Picked, present: ReadonlySet<string>): Picked {
  if (picked.paths.size === 0 && picked.anchor === null && picked.active === null) return picked;
  const kept = [...picked.paths].filter((path) => present.has(path));
  const anchor = picked.anchor !== null && present.has(picked.anchor) ? picked.anchor : null;
  const active = picked.active !== null && present.has(picked.active) ? picked.active : null;
  if (kept.length === picked.paths.size && anchor === picked.anchor && active === picked.active) return picked;
  return { paths: new Set(kept), anchor, active };
}

export function useResultSelection(hits: SearchHit[]) {
  const [picked, setPicked] = useState<Picked>(NOTHING_PICKED);
  const order = useMemo(() => hits.map((hit) => hit.path), [hits]);
  const present = useMemo(() => new Set(order), [order]);
  const current = useMemo(() => keep(picked, present), [picked, present]);
  useEffect(() => {
    if (current !== picked) setPicked(current);
  }, [current, picked]);

  const now = useLatest({ order, present });
  const pickPath = useCallback(
    (path: string, how: Pick) => setPicked((state) => pick(keep(state, now.current.present), now.current.order, path, how)),
    [now],
  );
  const select = useCallback((path: string, toggle: boolean, range = false) => pickPath(path, howToPick(toggle, range)), [pickPath]);
  const move = useCallback((path: string, extend: boolean) => pickPath(path, extend ? "range" : "replace"), [pickPath]);
  const selectAll = useCallback(() => setPicked((state) => ({ ...state, paths: new Set(now.current.order) })), [now]);
  const clear = useCallback(() => setPicked(NOTHING_PICKED), []);
  return { selected: current.paths, active: current.active, select, move, selectAll, clear };
}

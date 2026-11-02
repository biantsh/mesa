import { useCallback, useEffect, useMemo, useState } from "react";
import type { SearchHit } from "./api";
import { clicked } from "./selection";

const NOTHING: ReadonlySet<string> = new Set();

function keep(chosen: ReadonlySet<string>, hits: SearchHit[]): ReadonlySet<string> {
  if (chosen.size === 0) return chosen;
  const present = new Set(hits.map((hit) => hit.path));
  const kept = [...chosen].filter((path) => present.has(path));
  return kept.length === chosen.size ? chosen : new Set(kept);
}

export function useResultSelection(hits: SearchHit[]) {
  const [chosen, setChosen] = useState(NOTHING);
  const selected = useMemo(() => keep(chosen, hits), [chosen, hits]);
  useEffect(() => {
    if (selected !== chosen) setChosen(selected);
  }, [selected, chosen]);

  const select = useCallback((path: string, toggle: boolean) => setChosen((current) => clicked(current, path, toggle)), []);
  const selectAll = useCallback(() => setChosen(new Set(hits.map((hit) => hit.path))), [hits]);
  const clear = useCallback(() => setChosen(NOTHING), []);
  return { selected, select, selectAll, clear };
}

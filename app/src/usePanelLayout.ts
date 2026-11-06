import { useEffect, useRef, useState } from "react";

export type ListName = "indexed" | "excluded";

export interface Counts {
  indexed: number;
  excluded: number;
}

export function nextChoice(chosen: ListName | null, was: Counts, now: Counts): ListName | null {
  if (now.indexed === 0 && now.excluded === 0) return null;
  if (now.indexed > was.indexed) return "indexed";
  if (now.excluded > was.excluded) return "excluded";
  if (chosen !== null && now[chosen] === 0 && was[chosen] > 0) return null;
  return chosen;
}

export function usePanelLayout(indexed: number, excluded: number, ready: boolean) {
  const [chosen, setChosen] = useState<ListName | null>(null);
  const before = useRef<Counts | null>(null);
  useEffect(() => {
    if (!ready) return;
    const now = { indexed, excluded };
    const was = before.current ?? now;
    before.current = now;
    setChosen((current) => nextChoice(current, was, now));
  }, [indexed, excluded, ready]);

  const open: ListName = chosen ?? (indexed === 0 && excluded > 0 ? "excluded" : "indexed");

  return { open, choose: setChosen };
}

import { useCallback, useEffect, useRef, useState } from "react";
import { api, type ListStates, type Listed } from "./api";
import { usePoll } from "./usePoll";

const NOTHING: Listed[] = [];

interface Lists {
  indexed: Listed[];
  excluded: Listed[];
}

export function useIndexLists(trigger: string, active: boolean) {
  const [lists, setLists] = useState<Lists>({ indexed: NOTHING, excluded: NOTHING });
  const [loaded, setLoaded] = useState(false);
  const asked = useRef(0);
  const applied = useRef(0);
  const version = useRef<number | null>(null);

  const refresh = useCallback(async () => {
    const mine = ++asked.current;
    try {
      const read = await api.indexLists();
      if (mine < applied.current) return;
      applied.current = mine;
      version.current = read.version;
      setLists((current) => ({
        indexed: sameItems(current.indexed, read.indexed) ? current.indexed : read.indexed,
        excluded: sameItems(current.excluded, read.excluded) ? current.excluded : read.excluded,
      }));
      setLoaded(true);
    } catch {
    }
  }, []);

  const syncStates = useCallback(async () => {
    const known = version.current;
    if (known === null) return;
    let answer: ListStates;
    try {
      answer = await api.indexListStates(known);
    } catch {
      return;
    }
    if (version.current !== known) return;
    if (answer.states === null) return void refresh();
    const states = answer.states;
    setLists((current) => {
      const indexed = withStates(current.indexed, states);
      return indexed === current.indexed ? current : { ...current, indexed };
    });
  }, [refresh]);

  useEffect(() => {
    if (active) void refresh();
  }, [active, refresh]);

  useEffect(() => {
    if (active) void syncStates();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [trigger]);

  usePoll(syncStates, { active: active && lists.indexed.some(isUnfinished) });

  return { ...lists, loaded, refresh };
}

export function withStates(indexed: Listed[], states: ListStates["states"] & object): Listed[] {
  let changed = false;
  const next = indexed.map((item) => {
    const state = states[item.slot];
    if (!state) return item;
    const [index, droppable, removable] = state;
    if (item.index === index && item.droppable === droppable && item.removable === removable) return item;
    changed = true;
    return { ...item, index, droppable, removable };
  });
  return changed ? next : indexed;
}

const isUnfinished = (item: Listed) => item.index === "indexing" || item.index === "paused" || item.droppable;

export function sameItems(a: readonly Listed[], b: readonly Listed[]): boolean {
  if (a.length !== b.length) return false;
  for (let i = 0; i < a.length; i++) {
    const x = a[i] as unknown as Record<string, unknown>;
    const y = b[i] as unknown as Record<string, unknown>;
    if (x === y) continue;
    const keys = Object.keys(x);
    if (keys.length !== Object.keys(y).length) return false;
    for (const key of keys) if (x[key] !== y[key]) return false;
  }
  return true;
}

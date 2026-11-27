import { useCallback, useEffect, useRef, useState } from "react";
import { api, type ListStates, type Listed } from "./api";
import { useLatest } from "./useLatest";

const NOTHING: Listed[] = [];

interface Lists {
  indexed: Listed[];
  excluded: Listed[];
}

export function useIndexLists(active: boolean, indexing: boolean) {
  const [lists, setLists] = useState<Lists>({ indexed: NOTHING, excluded: NOTHING });
  const [loaded, setLoaded] = useState(false);
  const version = useRef<number | null>(null);
  const reading = useRef<Promise<void> | null>(null);
  const read = useCallback(async () => {
    try {
      const read = await api.indexLists();
      version.current = read.version;
      setLists((current) => ({
        indexed: sameItems(current.indexed, read.indexed) ? current.indexed : read.indexed,
        excluded: sameItems(current.excluded, read.excluded) ? current.excluded : read.excluded,
      }));
      setLoaded(true);
    } catch {
    }
  }, []);
  const refresh = useCallback(() => {
    reading.current ??= read().finally(() => {
      reading.current = null;
    });
    return reading.current;
  }, [read]);

  useEffect(() => {
    void refresh();
  }, [refresh]);
  useEffect(() => {
    if (active) void refresh();
  }, [active, refresh]);

  const unfinished = lists.indexed.some(isUnfinished);
  const wanted = useLatest(unfinished || indexing);
  const ask = useCallback(
    (now: boolean) => (version.current === null ? null : { version: version.current, states: now || wanted.current }),
    [wanted],
  );
  const take = useCallback(
    (answer: ListStates) => {
      if (answer.version !== version.current) {
        if (answer.stale) void refresh();
        return;
      }
      const states = answer.states;
      if (!states) return;
      setLists((current) => {
        const indexed = withStates(current.indexed, states);
        return indexed === current.indexed ? current : { ...current, indexed };
      });
    },
    [refresh],
  );

  return { ...lists, loaded, refresh, ask, take };
}

export function withStates(indexed: Listed[], states: ListStates["states"] & object): Listed[] {
  let changed = false;
  const next = indexed.map((item) => {
    const state = states[item.slot];
    if (!state) return item;
    const [index, queued, removable] = state;
    if (item.index === index && item.queued === queued && item.removable === removable) return item;
    changed = true;
    return { ...item, index, queued, removable };
  });
  return changed ? next : indexed;
}

const isUnfinished = (item: Listed) => item.index === "indexing" || item.index === "paused" || item.queued;

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

import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { api, type Listed } from "./api";
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

  const refresh = useCallback(async () => {
    const mine = ++asked.current;
    try {
      const [indexed, excluded] = await Promise.all([api.indexedItems(), api.excludedItems()]);
      if (mine < applied.current) return;
      applied.current = mine;
      setLists((current) => ({
        indexed: same(current.indexed, indexed) ? current.indexed : indexed,
        excluded: same(current.excluded, excluded) ? current.excluded : excluded,
      }));
      setLoaded(true);
    } catch {
    }
  }, []);

  useEffect(() => {
    if (active) void refresh();
  }, [active, trigger, refresh]);

  const unfinished = useMemo(() => lists.indexed.filter(isUnfinished).map((item) => item.path), [lists.indexed]);
  usePoll(
    async () => {
      const states = await api.indexStatuses(unfinished);
      setLists((current) => {
        const byPath = new Map(unfinished.map((path, i) => [path, states[i]]));
        let changed = false;
        const indexed = current.indexed.map((item) => {
          const state = byPath.get(item.path);
          if (!state || (item.index === state.status && item.droppable === state.droppable)) return item;
          changed = true;
          return { ...item, index: state.status, droppable: state.droppable };
        });
        return changed ? { ...current, indexed } : current;
      });
    },
    { active: active && unfinished.length > 0 },
  );

  return { ...lists, loaded, refresh };
}

const isUnfinished = (item: Listed) => item.index === "indexing" || item.index === "paused" || item.droppable;

const same = (a: Listed[], b: Listed[]) => JSON.stringify(a) === JSON.stringify(b);

import { useCallback, useEffect, useRef, useState } from "react";
import { api, type Listed } from "./api";

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

  return { ...lists, loaded, refresh };
}

const same = (a: Listed[], b: Listed[]) => JSON.stringify(a) === JSON.stringify(b);

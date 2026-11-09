import { useCallback, useRef, useState } from "react";
import { api, type Overview } from "./api";
import { IndexSnapshot } from "./indexSnapshot";
import { usePoll } from "./usePoll";

export function useIndexOverview(active: boolean): Overview | null {
  return useIndexOverviewNow(active).overview;
}

export function useIndexOverviewNow(active: boolean) {
  const snapshot = useRef<IndexSnapshot | null>(null);
  snapshot.current ??= new IndexSnapshot();
  const [overview, setOverview] = useState<Overview | null>(null);

  const refresh = useCallback(async () => {
    const store = snapshot.current!;
    const asking = store.ask();
    const next = await api.indexOverview();
    if (store.takeOverview(asking, next)) setOverview(store.overview);
  }, []);

  const refreshProgress = useCallback(async () => {
    const store = snapshot.current!;
    const asking = store.ask();
    try {
      const next = await api.indexProgress();
      if (store.takeProgress(asking, next)) setOverview(store.overview);
    } catch {
    }
  }, []);

  usePoll(refresh, { active, immediately: true });
  return { overview, refresh, refreshProgress };
}

import { useCallback, useState } from "react";
import { api, sameProgress, type Overview } from "./api";
import { usePoll } from "./usePoll";

export function useIndexOverview(active: boolean): Overview | null {
  return useIndexOverviewNow(active).overview;
}

export function useIndexOverviewNow(active: boolean) {
  const [overview, setOverview] = useState<Overview | null>(null);
  const refresh = useCallback(async () => {
    const next = await api.indexOverview();
    setOverview((current) =>
      current &&
      current.empty === next.empty &&
      current.everythingIndexed === next.everythingIndexed &&
      current.files === next.files &&
      current.bytes === next.bytes &&
      sameProgress(current.progress, next.progress)
        ? current
        : next,
    );
  }, []);
  usePoll(refresh, { active, immediately: true });
  return { overview, refresh };
}

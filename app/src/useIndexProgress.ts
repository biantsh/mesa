import { useCallback, useState } from "react";
import { api, type Progress } from "./api";
import { usePoll } from "./usePoll";

function same(a: Progress | null, b: Progress | null): boolean {
  if (a === null || b === null) return a === b;
  return (
    a.name === b.name &&
    a.queued === b.queued &&
    a.counting === b.counting &&
    a.found === b.found &&
    a.done === b.done &&
    a.total === b.total &&
    a.paused === b.paused
  );
}

export function useIndexProgress() {
  const [progress, setProgress] = useState<Progress | null>(null);

  const refresh = useCallback(async () => {
    try {
      const next = await api.indexProgress();
      setProgress((current) => (same(current, next) ? current : next));
    } catch {
    }
  }, []);

  usePoll(refresh, { immediately: true });

  return { progress, refresh };
}

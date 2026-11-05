import { useCallback, useRef, useState } from "react";
import { api, sameProgress, type Progress } from "./api";
import { usePoll } from "./usePoll";

export function useIndexProgress() {
  const [progress, setProgress] = useState<Progress | null>(null);
  const asked = useRef(0);
  const applied = useRef(0);

  const refresh = useCallback(async () => {
    const mine = ++asked.current;
    try {
      const next = await api.indexProgress();
      if (mine < applied.current) return;
      applied.current = mine;
      setProgress((current) => (sameProgress(current, next) ? current : next));
    } catch {
    }
  }, []);

  usePoll(refresh, { immediately: true });

  return { progress, refresh };
}

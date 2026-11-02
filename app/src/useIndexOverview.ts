import { useState } from "react";
import { api, type Overview, type Progress } from "./api";
import { usePoll } from "./usePoll";

function sameProgress(a: Progress | null, b: Progress | null): boolean {
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

export function useIndexOverview(active: boolean): Overview | null {
  const [overview, setOverview] = useState<Overview | null>(null);
  usePoll(
    async () => {
      const next = await api.indexOverview();
      setOverview((current) =>
        current &&
        current.empty === next.empty &&
        current.files === next.files &&
        current.bytes === next.bytes &&
        sameProgress(current.progress, next.progress)
          ? current
          : next,
      );
    },
    { active, immediately: true },
  );
  return overview;
}

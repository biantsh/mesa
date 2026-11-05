import { useState } from "react";
import { api, sameProgress, type Overview } from "./api";
import { usePoll } from "./usePoll";

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

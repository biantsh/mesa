import { useState } from "react";
import { api, type ModelStatus } from "./api";
import { startup } from "./ready";
import { usePoll } from "./usePoll";

export function useModelStatus(): ModelStatus | null {
  const [status, setStatus] = useState<ModelStatus | null>(startup.modelStatus);
  usePoll(
    async () => {
      const next = await api.modelStatus();
      setStatus((current) => (current && same(current, next) ? current : next));
    },
    { active: status?.state !== "ready", ms: 250, immediately: true },
  );
  return status;
}

function same(a: ModelStatus, b: ModelStatus): boolean {
  if (a.state !== b.state) return false;
  if (a.state === "downloading" && b.state === "downloading") return a.done === b.done && a.total === b.total;
  if (a.state === "failed" && b.state === "failed") return a.reason === b.reason && a.setup === b.setup;
  return true;
}

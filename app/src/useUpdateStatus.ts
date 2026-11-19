import { useCallback, useEffect, useMemo, useState } from "react";
import { api, type UpdateStatus } from "./api";
import { UPDATE_STATUS } from "./events";
import { useTauriEvent } from "./useTauriEvent";

export function useUpdateStatus() {
  const [status, setStatus] = useState<UpdateStatus | null>(null);
  useEffect(() => {
    let cancelled = false;
    api
      .updateStatus()
      .then((found) => !cancelled && setStatus((current) => current ?? found))
      .catch(() => {});
    return () => {
      cancelled = true;
    };
  }, []);
  useTauriEvent<UpdateStatus>(UPDATE_STATUS, setStatus);
  const install = useCallback(() => void api.installUpdate().catch(() => {}), []);
  const check = useCallback(() => void api.checkForUpdates().catch(() => {}), []);
  return useMemo(() => ({ status, install, check }), [status, install, check]);
}

export function hasUpdate(status: UpdateStatus | null): boolean {
  return status !== null && (status.state === "available" || status.state === "downloading" || status.state === "installing");
}

export function downloadFraction(status: UpdateStatus): number | null {
  return status.total ? Math.min(1, status.done / status.total) : null;
}

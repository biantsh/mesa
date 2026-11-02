import { useCallback, useEffect, useState } from "react";
import { api, errorText } from "./api";

export function useGpuAcceleration() {
  const [enabled, setEnabled] = useState<boolean | null>(null);
  const [gpu, setGpu] = useState<string | null | undefined>(undefined);
  const [pending, setPending] = useState<boolean | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    api
      .getSettings()
      .then((settings) => !cancelled && setEnabled(settings.gpuAcceleration))
      .catch((e) => !cancelled && setError(`Couldn't read the settings. ${errorText(e)}`));
    api
      .embeddingGpu()
      .then((name) => !cancelled && setGpu(name))
      .catch(() => !cancelled && setGpu(null));
    return () => {
      cancelled = true;
    };
  }, []);

  const change = useCallback(async (next: boolean) => {
    setError(null);
    setPending(next);
    try {
      await api.setGpuAcceleration(next);
      setEnabled(next);
    } catch (e) {
      setError(errorText(e));
    } finally {
      setPending(null);
    }
  }, []);

  return { enabled, gpu, pending, error, change };
}

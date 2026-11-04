import { useCallback, useEffect, useState } from "react";
import { api, errorText } from "./api";

export function useTurboIndexing() {
  const [enabled, setEnabled] = useState<boolean | null>(null);
  const [pending, setPending] = useState<boolean | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    api
      .getSettings()
      .then((settings) => !cancelled && setEnabled(settings.turboIndexing))
      .catch((e) => !cancelled && setError(`Couldn't read the settings. ${errorText(e)}`));
    return () => {
      cancelled = true;
    };
  }, []);

  const change = useCallback(async (next: boolean) => {
    setError(null);
    setPending(next);
    try {
      await api.setTurboIndexing(next);
      setEnabled(next);
    } catch (e) {
      setError(errorText(e));
    } finally {
      setPending(null);
    }
  }, []);

  return { enabled, pending, error, change };
}

import { useCallback, useEffect, useState } from "react";
import { api, errorText, type Settings } from "./api";

export function useSwitchSetting(read: (settings: Settings) => boolean, write: (enabled: boolean) => Promise<void>) {
  const [enabled, setEnabled] = useState<boolean | null>(null);
  const [pending, setPending] = useState<boolean | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    api
      .getSettings()
      .then((settings) => !cancelled && setEnabled(read(settings)))
      .catch((e) => !cancelled && setError(`Couldn't read the settings. ${errorText(e)}`));
    return () => {
      cancelled = true;
    };
  }, []);

  const change = useCallback(
    async (next: boolean) => {
      setError(null);
      setPending(next);
      try {
        await write(next);
        setEnabled(next);
      } catch (e) {
        setError(errorText(e));
      } finally {
        setPending(null);
      }
    },
    [write],
  );

  return { enabled, pending, error, change };
}

import { useEffect, useState } from "react";
import { getVersion } from "@tauri-apps/api/app";

export function useAppVersion(): string | null {
  const [version, setVersion] = useState<string | null>(null);
  useEffect(() => {
    let cancelled = false;
    getVersion()
      .then((found) => !cancelled && setVersion(found))
      .catch(() => {});
    return () => {
      cancelled = true;
    };
  }, []);
  return version;
}

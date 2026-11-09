import { useCallback, useEffect, useState } from "react";
import { api, type Category } from "./api";

export function useIndexedTypes(active: boolean) {
  const [types, setTypes] = useState<Category[] | null>(null);

  const refresh = useCallback(async () => {
    try {
      setTypes(await api.getIndexedTypes());
    } catch {
    }
  }, []);

  useEffect(() => {
    if (active) void refresh();
  }, [active, refresh]);

  const set = useCallback(async (chosen: Category[]) => {
    const now = await api.setIndexedTypes(chosen);
    setTypes(now);
    return now;
  }, []);

  return { types, refresh, set };
}

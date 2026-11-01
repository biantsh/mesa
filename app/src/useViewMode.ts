import { useCallback, useState } from "react";

export type ViewMode = "grid" | "list";

const KEY = "mesa.viewMode";

function stored(): ViewMode {
  try {
    return localStorage.getItem(KEY) === "list" ? "list" : "grid";
  } catch {
    return "grid";
  }
}

export function useViewMode(): [ViewMode, (mode: ViewMode) => void] {
  const [mode, setMode] = useState<ViewMode>(stored);

  const change = useCallback((next: ViewMode) => {
    setMode(next);
    try {
      localStorage.setItem(KEY, next);
    } catch {
    }
  }, []);

  return [mode, change];
}

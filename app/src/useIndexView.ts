import { useCallback, useState } from "react";

export type IndexView = "simple" | "advanced";

const KEY = "mesa.indexView";

export function storedIndexView(): IndexView {
  try {
    return localStorage.getItem(KEY) === "advanced" ? "advanced" : "simple";
  } catch {
    return "simple";
  }
}

function keepIndexView(view: IndexView) {
  try {
    localStorage.setItem(KEY, view);
  } catch {
  }
}

export function useIndexView(): [IndexView, (view: IndexView) => void] {
  const [view, setView] = useState<IndexView>(storedIndexView);
  const change = useCallback((next: IndexView) => {
    setView(next);
    keepIndexView(next);
  }, []);
  return [view, change];
}

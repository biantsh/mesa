import { useCallback, useState } from "react";

export type Theme = "light" | "dark";

const KEY = "mesa.theme";

export function currentTheme(): Theme {
  return document.documentElement.dataset.theme === "dark" ? "dark" : "light";
}

const FADE_MS = 300;
const SETTLED_MS = FADE_MS + 50;
let settling: number | undefined;

function apply(theme: Theme) {
  const root = document.documentElement;
  root.dataset.themeChanging = "";
  root.dataset.theme = theme;
  window.clearTimeout(settling);
  settling = window.setTimeout(() => delete root.dataset.themeChanging, SETTLED_MS);
  try {
    localStorage.setItem(KEY, theme);
  } catch {
  }
}

export function useTheme(): [Theme, (theme: Theme) => void] {
  const [theme, setTheme] = useState<Theme>(currentTheme);
  const change = useCallback((next: Theme) => {
    apply(next);
    setTheme(next);
  }, []);
  return [theme, change];
}

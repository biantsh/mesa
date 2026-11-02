import { useCallback, useState } from "react";

export type Theme = "light" | "dark";

const KEY = "mesa.theme";

export function currentTheme(): Theme {
  return document.documentElement.dataset.theme === "dark" ? "dark" : "light";
}

function apply(theme: Theme) {
  document.documentElement.dataset.theme = theme;
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

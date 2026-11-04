import { useCallback, useState } from "react";
import { emit } from "@tauri-apps/api/event";

export type Theme = "light" | "dark";

const KEY = "mesa.theme";
export const THEME_EVENT = "theme-changed";

export function chosenTheme(): Theme | null {
  try {
    const kept = localStorage.getItem(KEY);
    return kept === "light" || kept === "dark" ? kept : null;
  } catch {
    return null;
  }
}

export function currentTheme(): Theme {
  return document.documentElement.dataset.theme === "dark" ? "dark" : "light";
}

const FADE_MS = 300;
const SETTLED_MS = FADE_MS + 50;
let settling: number | undefined;

export function followTheme(theme: Theme) {
  const root = document.documentElement;
  if (root.dataset.theme === theme) return;
  root.dataset.themeChanging = "";
  root.dataset.theme = theme;
  window.clearTimeout(settling);
  settling = window.setTimeout(() => delete root.dataset.themeChanging, SETTLED_MS);
}

function apply(theme: Theme) {
  followTheme(theme);
  try {
    localStorage.setItem(KEY, theme);
  } catch {
  }
  void emit(THEME_EVENT, theme).catch(() => {});
}

export function useTheme(): [Theme, (theme: Theme) => void] {
  const [theme, setTheme] = useState<Theme>(currentTheme);
  const change = useCallback((next: Theme) => {
    apply(next);
    setTheme(next);
  }, []);
  return [theme, change];
}

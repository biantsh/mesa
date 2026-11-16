import { useCallback, useEffect, useState } from "react";
import { emit } from "@tauri-apps/api/event";

export type Theme = "light" | "dark";
export type ThemeChoice = Theme | "system";

const KEY = "mesa.theme";
export const THEME_EVENT = "theme-changed";

const SYSTEM_DARK = "(prefers-color-scheme: dark)";

export function themeChoice(): ThemeChoice {
  try {
    const kept = localStorage.getItem(KEY);
    return kept === "light" || kept === "dark" ? kept : "system";
  } catch {
    return "system";
  }
}

export function themeOf(choice: ThemeChoice): Theme {
  if (choice !== "system") return choice;
  return typeof matchMedia !== "undefined" && matchMedia(SYSTEM_DARK).matches ? "dark" : "light";
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

function apply(choice: ThemeChoice) {
  const theme = themeOf(choice);
  followTheme(theme);
  try {
    if (choice === "system") localStorage.removeItem(KEY);
    else localStorage.setItem(KEY, choice);
  } catch {
  }
  void emit(THEME_EVENT, theme).catch(() => {});
}

export function useTheme(): [ThemeChoice, (choice: ThemeChoice) => void] {
  const [choice, setChoice] = useState<ThemeChoice>(themeChoice);
  const change = useCallback((next: ThemeChoice) => {
    apply(next);
    setChoice(next);
  }, []);
  return [choice, change];
}

export function useFollowSystemTheme() {
  useEffect(() => {
    if (typeof matchMedia === "undefined") return;
    const system = matchMedia(SYSTEM_DARK);
    const follow = () => {
      if (themeChoice() === "system") followTheme(themeOf("system"));
    };
    system.addEventListener("change", follow);
    return () => system.removeEventListener("change", follow);
  }, []);
}

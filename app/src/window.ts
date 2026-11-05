import { useEffect } from "react";
import { getCurrentWindow, type Window } from "@tauri-apps/api/window";
import { api } from "./api";

export function currentWindow(): Window | null {
  try {
    return getCurrentWindow();
  } catch {
    return null;
  }
}

export const windowControls = {
  minimize: () => void currentWindow()?.minimize().catch(() => {}),
  toggleMaximize: () => void currentWindow()?.toggleMaximize().catch(() => {}),
  close: () => void currentWindow()?.close().catch(() => {}),
};

export function showWhenPainted() {
  requestAnimationFrame(() =>
    requestAnimationFrame(() => {
      if (currentWindow()) void api.pagePainted().catch(() => {});
    }),
  );
}

export function useBlockContextMenu() {
  useEffect(() => {
    const block = (e: MouseEvent) => e.preventDefault();
    window.addEventListener("contextmenu", block);
    return () => window.removeEventListener("contextmenu", block);
  }, []);
}

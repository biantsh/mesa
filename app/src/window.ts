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

export function fitTheWindowExactly() {
  const window_ = currentWindow();
  if (!window_) return;
  const root = document.documentElement;
  const measure = () =>
    void Promise.all([window_.innerSize(), window_.scaleFactor()])
      .then(([size, scale]) => {
        if (scale <= 0) return;
        root.style.setProperty("--window-width", `${size.width / scale}px`);
        root.style.setProperty("--window-height", `${size.height / scale}px`);
      })
      .catch(() => {});
  measure();
  let timer = 0;
  window.addEventListener("resize", () => {
    window.clearTimeout(timer);
    timer = window.setTimeout(measure, RESIZE_SETTLED_MS);
  });
}

const RESIZE_SETTLED_MS = 150;

export const windowControls = {
  minimize: () => void currentWindow()?.minimize().catch(() => {}),
  toggleMaximize: () => void currentWindow()?.toggleMaximize().catch(() => {}),
  startDragging: () => void currentWindow()?.startDragging().catch(() => {}),
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

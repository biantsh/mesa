import { useEffect, useRef, useState } from "react";
import { flushSync } from "react-dom";
import { api } from "./api";
import { FOCUS_SEARCH, NAVIGATE } from "./events";
import { useTauriEvent } from "./useTauriEvent";

const FROM_WIDGET_MS = 450;

interface Window {
  setScreen: (screen: "home" | "index") => void;
  setSettingsOpen: (open: boolean) => void;
  focusSearch: () => void;
}

export function useWidgetRequests({ setScreen, setSettingsOpen, focusSearch }: Window): boolean {
  const [fromWidget, setFromWidget] = useState(false);
  const timer = useRef(0);
  useEffect(() => () => window.clearTimeout(timer.current), []);

  useTauriEvent<string>(NAVIGATE, (screen) => {
    flushSync(() => {
      if (screen === "index") {
        setFromWidget(true);
        setScreen("index");
      } else if (screen === "settings") {
        setScreen("home");
        setSettingsOpen(true);
      }
    });
    window.clearTimeout(timer.current);
    timer.current = window.setTimeout(() => setFromWidget(false), FROM_WIDGET_MS);
    void api.showMain().catch(() => {});
  });

  useTauriEvent(FOCUS_SEARCH, () => {
    setScreen("home");
    setSettingsOpen(false);
    requestAnimationFrame(() => requestAnimationFrame(focusSearch));
  });

  return fromWidget;
}

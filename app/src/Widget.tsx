import { useCallback, useEffect, useRef, useState } from "react";
import { api } from "./api";
import { THEME_EVENT, chosenTheme, followTheme, type Theme } from "./theme";
import { HomeScreen, type HomeHandle } from "./screens/HomeScreen";
import { useIndexOverview } from "./useIndexOverview";
import { useModelStatus } from "./useModelStatus";
import { useTauriEvent } from "./useTauriEvent";
import { useBlockContextMenu } from "./window";
import styles from "./Widget.module.css";

const BOX_MARGIN_PX = 2;

type Box = [number, number, number, number];

function visibleBox(): Box | null {
  const backdrop = document.querySelector<HTMLElement>("[data-widget-backdrop]");
  const spot = document.querySelector<HTMLElement>("[data-widget-spot]");
  const seen = backdrop && Number(getComputedStyle(backdrop).opacity) > 0.01 ? backdrop : spot;
  if (!seen) return null;
  const r = seen.getBoundingClientRect();
  return [r.left - BOX_MARGIN_PX, r.top - BOX_MARGIN_PX, r.right + BOX_MARGIN_PX, r.bottom + BOX_MARGIN_PX].map(Math.round) as Box;
}

export default function Widget() {
  const [shown, setShown] = useState(0);
  const [there, setThere] = useState(false);
  const home = useRef<HomeHandle>(null);
  const frame = useRef(0);
  const overview = useIndexOverview(true);
  const model = useModelStatus();
  const retryModel = useCallback(() => void api.retryModelSetup().catch(() => {}), []);
  const openIndex = useCallback(() => void api.openMain("index").catch(() => {}), []);
  const openSettings = useCallback(() => void api.openMain("settings").catch(() => {}), []);
  const hide = useCallback(() => void api.hideWidget().catch(() => {}), []);

  useTauriEvent("widget-hidden", () => {
    cancelAnimationFrame(frame.current);
    setThere(false);
  });
  useTauriEvent("widget-shown", () => {
    const theme = chosenTheme();
    if (theme) followTheme(theme);
    cancelAnimationFrame(frame.current);
    frame.current = requestAnimationFrame(() => {
      setShown((now) => now + 1);
      setThere(true);
    });
  });
  useEffect(() => () => cancelAnimationFrame(frame.current), []);
  useTauriEvent<Theme>(THEME_EVENT, followTheme);

  useEffect(() => {
    if (!there) return;
    let second = 0;
    const first = requestAnimationFrame(() => {
      second = requestAnimationFrame(() => home.current?.focusField());
    });
    return () => {
      cancelAnimationFrame(first);
      cancelAnimationFrame(second);
    };
  }, [there, shown]);

  useEffect(() => {
    if (!there) return;
    let frame = 0;
    let said = "";
    const look = () => {
      const box = visibleBox();
      const text = box?.join(",") ?? "";
      if (box && text !== said) {
        said = text;
        void api.setWidgetBox(...box).catch(() => {});
      }
      frame = requestAnimationFrame(look);
    };
    frame = requestAnimationFrame(look);
    return () => cancelAnimationFrame(frame);
  }, [there]);

  useBlockContextMenu();

  return (
    <div
      className={styles.shell}
      onMouseDownCapture={(e) => {
        if ((e.target as HTMLElement).closest("button")) e.preventDefault();
      }}
    >
      <div className={styles.screen}>
        {there && (
          <HomeScreen
            key={shown}
            ref={home}
            overview={overview}
            model={model}
            onRetryModel={retryModel}
            onOpenIndex={openIndex}
            onOpenSettings={openSettings}
            asWidget
            onDismiss={hide}
          />
        )}
      </div>
    </div>
  );
}

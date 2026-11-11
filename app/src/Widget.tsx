import { useCallback, useEffect, useRef, useState } from "react";
import { api } from "./api";
import { WIDGET_HIDDEN, WIDGET_SHOWN } from "./events";
import { THEME_EVENT, chosenTheme, followTheme, type Theme } from "./theme";
import { HomeScreen, type HomeHandle } from "./screens/HomeScreen";
import { useIndexOverview } from "./useIndexOverview";
import { useModelStatus } from "./useModelStatus";
import { useTauriEvent } from "./useTauriEvent";
import { useBlockContextMenu } from "./window";
import { reportVisibleBox } from "./widgetBox";
import styles from "./Widget.module.css";

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

  useTauriEvent(WIDGET_HIDDEN, () => {
    cancelAnimationFrame(frame.current);
    setThere(false);
  });
  useTauriEvent(WIDGET_SHOWN, () => {
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
    return reportVisibleBox((box) => void api.setWidgetBox(...box).catch(() => {}));
  }, [there]);

  useEffect(() => {
    document.documentElement.dataset.widget = "";
    api
      .widgetFillsScreen()
      .then((fills) => {
        if (fills) document.documentElement.dataset.widgetFill = "";
      })
      .catch(() => {});
  }, []);

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

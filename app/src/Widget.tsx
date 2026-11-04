import { useCallback, useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { api } from "./api";
import { THEME_EVENT, chosenTheme, followTheme, type Theme } from "./theme";
import { HomeScreen, type HomeHandle } from "./screens/HomeScreen";
import { useIndexOverview } from "./useIndexOverview";
import { useModelStatus } from "./useModelStatus";
import styles from "./Widget.module.css";

const OPEN_ON_TOP = "dialog[open], [role='menu']";

export default function Widget() {
  const [shown, setShown] = useState(0);
  const [there, setThere] = useState(false);
  const home = useRef<HomeHandle>(null);
  const overview = useIndexOverview(true);
  const model = useModelStatus();
  const retryModel = useCallback(() => void api.retryModelSetup().catch(() => {}), []);
  const openIndex = useCallback(() => void api.openMain("index").catch(() => {}), []);
  const openSettings = useCallback(() => void api.openMain("settings").catch(() => {}), []);
  const hide = useCallback(() => void api.hideWidget().catch(() => {}), []);

  useEffect(() => {
    let frame = 0;
    const hidden = listen("widget-hidden", () => {
      cancelAnimationFrame(frame);
      setThere(false);
    });
    const shownNow = listen("widget-shown", () => {
      const theme = chosenTheme();
      if (theme) followTheme(theme);
      cancelAnimationFrame(frame);
      frame = requestAnimationFrame(() => {
        setShown((now) => now + 1);
        setThere(true);
      });
    });
    return () => {
      cancelAnimationFrame(frame);
      void hidden.then((stop) => stop()).catch(() => {});
      void shownNow.then((stop) => stop()).catch(() => {});
    };
  }, []);
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
    const listening = listen<Theme>(THEME_EVENT, (event) => followTheme(event.payload));
    return () => void listening.then((stop) => stop()).catch(() => {});
  }, []);

  useEffect(() => {
    const onKeyDown = (e: KeyboardEvent) => {
      if (e.key !== "Escape" || document.querySelector(OPEN_ON_TOP)) return;
      setTimeout(() => {
        if (!e.defaultPrevented) hide();
      }, 0);
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [hide]);

  useEffect(() => {
    const block = (e: MouseEvent) => e.preventDefault();
    window.addEventListener("contextmenu", block);
    return () => window.removeEventListener("contextmenu", block);
  }, []);

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
            onFocusChange={noop}
            onOpenIndex={openIndex}
            onOpenSettings={openSettings}
            onFileOpened={hide}
            asWidget
            onLeftEmpty={hide}
            keepFocusWhenCleared
          />
        )}
      </div>
    </div>
  );
}

function noop() {}

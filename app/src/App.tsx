import { useCallback, useEffect, useRef, useState } from "react";
import { SettingsDialog } from "./components/SettingsDialog";
import { SetupScreen } from "./components/SetupScreen";
import { TitleBar } from "./components/TitleBar";
import { cx } from "./cx";
import { HomeScreen, type HomeHandle } from "./screens/HomeScreen";
import { IndexScreen } from "./screens/IndexScreen";
import { api, type ModelStatus } from "./api";
import { startup } from "./ready";
import { useIndexOverview } from "./useIndexOverview";
import { useMaximized } from "./useMaximized";
import { useTauriEvent } from "./useTauriEvent";
import { useBlockContextMenu } from "./window";
import { useModelStatus } from "./useModelStatus";
import styles from "./App.module.css";

type Screen = "home" | "index";
const SETUP_FADE_MS = 300;

function needsSetup(status: ModelStatus | null): boolean {
  return status?.state === "downloading" || (status?.state === "failed" && status.setup);
}

const OPEN_ON_TOP = "dialog[open], [role='menu']";

export default function App() {
  const [screen, setScreen] = useState<Screen>("home");
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [querying, setQuerying] = useState(false);
  const onIndex = screen === "index";
  const dimmed = querying && !onIndex;
  const maximized = useMaximized();
  const overview = useIndexOverview(!onIndex);
  const model = useModelStatus();
  const [setup, setSetup] = useState<"off" | "on" | "leaving">(needsSetup(startup.modelStatus) ? "on" : "off");
  useEffect(() => {
    if (setup === "off" && needsSetup(model)) setSetup("on");
    else if (setup === "on" && model?.state === "ready") setSetup("leaving");
  }, [model, setup]);
  useEffect(() => {
    if (setup !== "leaving") return;
    const timer = setTimeout(() => setSetup("off"), SETUP_FADE_MS);
    return () => clearTimeout(timer);
  }, [setup]);
  const covered = setup !== "off";
  const retryModel = useCallback(() => void api.retryModelSetup().catch(() => {}), []);

  const home = useRef<HTMLDivElement>(null);
  const homeScreen = useRef<HomeHandle>(null);
  const index = useRef<HTMLDivElement>(null);
  const goHome = useCallback(() => setScreen("home"), []);
  const goToIndex = useCallback(() => setScreen("index"), []);
  const openSettings = useCallback(() => setSettingsOpen(true), []);
  const closeSettings = useCallback(() => setSettingsOpen(false), []);
  const first = useRef(true);
  useEffect(() => {
    if (first.current) {
      first.current = false;
      return;
    }
    if (!onIndex && homeScreen.current?.restoreFocus()) return;
    (onIndex ? index : home).current?.focus({ preventScroll: true });
  }, [onIndex]);

  useTauriEvent<string>("navigate", (screen) => {
    if (screen === "index") setScreen("index");
    else if (screen === "settings") {
      setScreen("home");
      setSettingsOpen(true);
    }
  });

  useBlockContextMenu();

  useEffect(() => {
    if (!onIndex) return;
    const onKeyDown = (e: KeyboardEvent) => {
      const back = e.key === "Escape" || (e.altKey && e.key === "ArrowLeft");
      if (!back || document.querySelector(OPEN_ON_TOP)) return;
      e.preventDefault();
      goHome();
    };
    const onMouseUp = (e: MouseEvent) => {
      if (e.button !== 3 || document.querySelector(OPEN_ON_TOP)) return;
      e.preventDefault();
      goHome();
    };
    window.addEventListener("keydown", onKeyDown);
    window.addEventListener("mouseup", onMouseUp);
    return () => {
      window.removeEventListener("keydown", onKeyDown);
      window.removeEventListener("mouseup", onMouseUp);
    };
  }, [onIndex, goHome]);

  return (
    <div className={cx(styles.window, dimmed && styles.dimmedWindow)}>
      <div className={styles.titleArea}>
        <TitleBar maximized={maximized} />
        <div className={cx(styles.titleDim, dimmed && styles.dimmed)} aria-hidden />
      </div>
      <div className={styles.screens}>
        <div
          ref={home}
          className={cx(styles.pane, onIndex && styles.before)}
          tabIndex={-1}
          inert={onIndex || covered}
        >
          <HomeScreen
            ref={homeScreen}
            overview={overview}
            model={model}
            onRetryModel={retryModel}
            onFocusChange={setQuerying}
            onOpenIndex={goToIndex}
            onOpenSettings={openSettings}
          />
        </div>
        <div
          ref={index}
          className={cx(styles.pane, !onIndex && styles.after)}
          tabIndex={-1}
          inert={!onIndex || covered}
        >
          <IndexScreen onBack={goHome} active={onIndex} />
        </div>
        {covered && model && <SetupScreen status={model} leaving={setup === "leaving"} />}
      </div>
      <SettingsDialog open={settingsOpen} onClose={closeSettings} />
    </div>
  );
}

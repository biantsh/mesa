import { useCallback, useEffect, useRef, useState } from "react";
import { SettingsDialog } from "./components/SettingsDialog";
import { SetupScreen } from "./components/SetupScreen";
import { TitleBar } from "./components/TitleBar";
import { cx } from "./cx";
import { HomeScreen } from "./screens/HomeScreen";
import { IndexScreen } from "./screens/IndexScreen";
import { api, type ModelStatus } from "./api";
import { startup } from "./ready";
import { useIndexOverview } from "./useIndexOverview";
import { useMaximized } from "./useMaximized";
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
  const onIndex = screen === "index";
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
    (onIndex ? index : home).current?.focus({ preventScroll: true });
  }, [onIndex]);

  useEffect(() => {
    const block = (e: MouseEvent) => e.preventDefault();
    window.addEventListener("contextmenu", block);
    return () => window.removeEventListener("contextmenu", block);
  }, []);

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
    <div className={styles.window}>
      <TitleBar maximized={maximized} />
      <div className={styles.screens}>
        <div
          ref={home}
          className={cx(styles.pane, onIndex && styles.before)}
          tabIndex={-1}
          inert={onIndex || covered}
        >
          <HomeScreen
            overview={overview}
            model={model}
            onRetryModel={retryModel}
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

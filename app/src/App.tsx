import { memo, useCallback, useEffect, useRef, useState } from "react";
import { SettingsDialog } from "./components/SettingsDialog";
import { SetupScreen } from "./components/SetupScreen";
import { TitleBar } from "./components/TitleBar";
import { cx } from "./cx";
import { MAIN_HIDDEN, MAIN_SHOWN } from "./events";
import { isMac } from "./platform";
import { HomeScreen, type HomeHandle } from "./screens/HomeScreen";
import { IndexPage } from "./screens/IndexPage";
import { api, type ModelStatus } from "./api";
import { describeIndexTotals } from "./format";
import { LegalCorner } from "./components/LegalCorner";
import { startup } from "./ready";
import { useCornersAway } from "./useCornersAway";
import { useFrozenWhile } from "./useFrozenWhile";
import { useIndexOverviewNow } from "./useIndexOverview";
import { useFullscreen, useMaximized } from "./useMaximized";
import { useTauriEvent } from "./useTauriEvent";
import { useBlockContextMenu } from "./window";
import { useModelStatus } from "./useModelStatus";
import { useWidgetRequests } from "./useWidgetRequests";
import styles from "./App.module.css";

type Screen = "home" | "index";
const SETUP_FADE_MS = 300;

function needsSetup(status: ModelStatus | null): boolean {
  return status?.state === "downloading" || (status?.state === "failed" && status.setup);
}

const OPEN_ON_TOP = "dialog[open], [role='menu']";
const Home = memo(HomeScreen);
const FOCUS_DELAY_MS = 0;

export default function App() {
  const [screen, setScreen] = useState<Screen>("home");
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [querying, setQuerying] = useState(false);
  const onIndex = screen === "index";
  const dimmed = (querying && !onIndex) || settingsOpen;
  const maximized = useMaximized();
  useFullscreen();
  const { overview, refresh: refreshOverview } = useIndexOverviewNow(true);
  const homeOverview = useFrozenWhile(onIndex, overview);
  const [pillWidth, setPillWidth] = useState<number | null>(null);
  const screensBox = useRef<HTMLDivElement>(null);
  const totalsLine = useRef<HTMLElement>(null);
  const legalLine = useRef<HTMLElement>(null);
  const cornersAway = useCornersAway(screensBox, totalsLine, legalLine, pillWidth);
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
  const behind = covered || settingsOpen;
  const retryModel = useCallback(() => void api.retryModelSetup().catch(() => {}), []);

  const home = useRef<HTMLDivElement>(null);
  const homeScreen = useRef<HomeHandle>(null);
  const index = useRef<HTMLDivElement>(null);
  const goHome = useCallback(() => setScreen("home"), []);
  const goToIndex = useCallback(() => setScreen("index"), []);
  const focusOnArrival = useRef(false);
  const searchFromIndex = useCallback(() => {
    if (homeScreen.current?.hasText()) focusOnArrival.current = true;
    else window.setTimeout(() => homeScreen.current?.focusField(), FOCUS_DELAY_MS);
    setScreen("home");
  }, []);
  const beforeSettings = useRef<Element | null>(null);
  const changeSettingsOpen = useCallback((open: boolean) => {
    if (open) beforeSettings.current = document.activeElement;
    setSettingsOpen(open);
  }, []);
  const openSettings = useCallback(() => changeSettingsOpen(true), [changeSettingsOpen]);
  const closeSettings = useCallback(() => changeSettingsOpen(false), [changeSettingsOpen]);
  useEffect(() => {
    const back = beforeSettings.current;
    if (settingsOpen || !back) return;
    beforeSettings.current = null;
    const at = document.activeElement;
    if (back instanceof HTMLElement && back.isConnected && (!at || at === document.body || at.closest("dialog"))) {
      back.focus({ preventScroll: true });
    }
  }, [settingsOpen]);

  const first = useRef(true);
  useEffect(() => {
    if (first.current) {
      first.current = false;
      return;
    }
    if (!onIndex && focusOnArrival.current) {
      focusOnArrival.current = false;
      homeScreen.current?.focusFieldAtOnce();
      return;
    }
    if (!onIndex && homeScreen.current?.restoreFocus()) return;
    (onIndex ? index : home).current?.focus({ preventScroll: true });
  }, [onIndex]);

  const fromWidget = useWidgetRequests({
    setScreen,
    setSettingsOpen: changeSettingsOpen,
    focusSearch: () => homeScreen.current?.focusField(),
  });

  useTauriEvent(MAIN_HIDDEN, () => {
    setScreen("home");
    setSettingsOpen(false);
  });

  useTauriEvent(MAIN_SHOWN, () => {
    requestAnimationFrame(() => requestAnimationFrame(() => void api.revealMain().catch(() => {})));
  });

  useBlockContextMenu();

  useEffect(() => {
    if (!onIndex) return;
    const onKeyDown = (e: KeyboardEvent) => {
      const back = e.key === "Escape" || (e.altKey && e.key === "ArrowLeft") || (isMac && e.metaKey && e.key === "[");
      if (!back || e.defaultPrevented || document.querySelector(OPEN_ON_TOP)) return;
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
      <div ref={screensBox} className={cx(styles.screens, fromWidget && styles.fromWidget)}>
        <div
          ref={home}
          className={cx(styles.pane, onIndex && styles.before)}
          tabIndex={-1}
          inert={onIndex || behind}
        >
          <Home
            ref={homeScreen}
            overview={homeOverview}
            model={model}
            onRetryModel={retryModel}
            onFocusChange={setQuerying}
            onBottomPill={setPillWidth}
            onScreen={!onIndex && !behind}
            indexShown={onIndex}
            onOpenIndex={goToIndex}
            onOpenSettings={openSettings}
          />
        </div>
        <div
          ref={index}
          className={cx(styles.pane, !onIndex && styles.after, onIndex && fromWidget && styles.arrive)}
          tabIndex={-1}
          inert={!onIndex || behind}
        >
          <IndexPage onBack={goHome} onSearch={searchFromIndex} overview={overview} onRefreshOverview={refreshOverview} active={onIndex} />
        </div>
        <footer ref={totalsLine} className={cx(styles.totals, cornersAway && styles.cornerAway)}>
          {overview && describeIndexTotals(overview.files, overview.bytes)}
        </footer>
        <footer ref={legalLine} className={cx(styles.legalCorner, cornersAway && styles.cornerAway)}>
          <LegalCorner />
        </footer>
        {covered && model && <SetupScreen status={model} leaving={setup === "leaving"} />}
        <SettingsDialog open={settingsOpen} onClose={closeSettings} />
      </div>
    </div>
  );
}

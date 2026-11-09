import { useSecondsLeft } from "../useSecondsLeft";
import { useCallback, useEffect, useRef } from "react";
import type { Overview } from "../api";
import { changeWithTransition } from "../viewTransition";
import { IndexScreen } from "./IndexScreen";
import { SimpleIndexScreen } from "./SimpleIndexScreen";
import { useIndexView } from "../useIndexView";
import styles from "./IndexPage.module.css";

interface Props {
  onBack: () => void;
  onSearch: () => void;
  overview: Overview | null;
  onRefreshOverview: () => Promise<void>;
  onRefreshProgress: () => Promise<void>;
  active: boolean;
}

const ARRIVAL_DELAY_MS = 100;

export function IndexPage({ onBack, onSearch, overview, onRefreshOverview, onRefreshProgress, active }: Props) {
  const secondsLeft = useSecondsLeft(overview?.progress ?? null);
  const [view, setView] = useIndexView();
  const simple = view === "simple";
  const wasActive = useRef(active);
  const arrivalDelay = wasActive.current ? ARRIVAL_DELAY_MS : 0;
  useEffect(() => {
    wasActive.current = active;
  });
  const showSimple = useCallback(() => changeWithTransition(() => setView("simple")), [setView]);
  const showAdvanced = useCallback(() => changeWithTransition(() => setView("advanced")), [setView]);
  const simplePage = useRef<HTMLDivElement>(null);
  const advancedPage = useRef<HTMLDivElement>(null);
  const first = useRef(true);
  useEffect(() => {
    if (first.current) {
      first.current = false;
      return;
    }
    (simple ? simplePage : advancedPage).current?.focus({ preventScroll: true });
  }, [simple]);

  return (
    <>
      <div ref={simplePage} className={styles.view} tabIndex={-1} data-off={!simple ? "" : undefined} inert={!simple}>
        <SimpleIndexScreen
          onBack={onBack}
          onAdvanced={showAdvanced}
          onSearch={onSearch}
          overview={overview}
          secondsLeft={secondsLeft}
          onRefreshOverview={onRefreshOverview}
          onRefreshProgress={onRefreshProgress}
          active={active && simple}
        />
      </div>
      <div ref={advancedPage} className={styles.view} tabIndex={-1} data-off={simple ? "" : undefined} inert={simple}>
        <IndexScreen
          onBack={onBack}
          onSimple={showSimple}
          overview={overview}
          onRefreshProgress={onRefreshProgress}
          secondsLeft={secondsLeft}
          arrivalDelay={arrivalDelay}
          active={active && !simple}
        />
      </div>
    </>
  );
}

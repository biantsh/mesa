import { useEffect, useMemo, useState, type ReactNode } from "react";
import { DatabaseZap, Info, Search, Trash2 } from "lucide-react";
import { api, errorText, type Category, type Estimate, type Overview, type Survey } from "../api";
import { describeTotal, estimateSeconds, formatEstimateRange, joinNames } from "../fileSurvey";
import { Button } from "../components/Button";
import { ConfirmDialog } from "../components/ConfirmDialog";
import { IndexHeader } from "../components/IndexHeader";
import { SimpleProgress } from "../components/SimpleProgress";
import { TypeTiles } from "../components/TypeTiles";
import { DEFAULT_TYPES, TYPE_ORDER, describeTypes, hasNewTypes, typesToIndex } from "../indexTypes";
import { HINT_DELAY_MS, hasShownBackgroundHint, markBackgroundHintShown } from "../backgroundHint";
import { SWITCH_NAME } from "../indexView";
import { MAIN_HIDDEN } from "../events";
import { useTauriEvent } from "../useTauriEvent";
import { changeWithTransition } from "../viewTransition";
import { useIndexedTypes } from "../useIndexedTypes";
import { cx } from "../cx";
import { prefersReducedMotion } from "../motion";
import { useLatest } from "../useLatest";
import styles from "./SimpleIndexScreen.module.css";

type Mode = "setup" | "running" | "idle";

interface Props {
  onBack: () => void;
  onAdvanced: () => void;
  onSearch: () => void;
  overview: Overview | null;
  secondsLeft: number | null;
  onRefresh: () => Promise<void>;
  active: boolean;
}

export function SimpleIndexScreen({
  onBack,
  onAdvanced,
  onSearch,
  overview,
  secondsLeft,
  onRefresh: refresh,
  active,
}: Props) {
  const progress = overview?.progress ?? null;
  const indexed = useIndexedTypes(active);
  const [picked, setPicked] = useState<Category[]>(DEFAULT_TYPES);
  const [starting, setStarting] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const running = progress !== null;
  const nothingIndexed = overview?.empty ?? true;
  const ready = overview !== null && indexed.types !== null;
  const mode: Mode = running ? "running" : nothingIndexed ? "setup" : "idle";
  const indexedTypes = indexed.types ?? [];
  const [settled, setSettled] = useState(false);
  const [shown, setShown] = useState<Mode>(mode);
  const view = settled ? shown : mode;
  useEffect(() => {
    if (ready && !settled) {
      setShown(mode);
      setSettled(true);
    }
  }, [ready, settled, mode]);
  useEffect(() => {
    if (!settled || mode === shown) return;
    if (active && shown === "setup" && mode === "running") changeWithTransition(() => setShown(mode), { onlyMorph: true });
    else setShown(mode);
  }, [settled, mode, shown, active]);
  const [hint, setHint] = useState(false);
  useEffect(() => {
    if (view !== "running") {
      setHint(false);
      return;
    }
    if (!active || hint || hasShownBackgroundHint()) return;
    const timer = setTimeout(() => {
      markBackgroundHintShown();
      setHint(true);
    }, HINT_DELAY_MS);
    return () => clearTimeout(timer);
  }, [view, active, hint]);

  const chosenTypes = indexed.types;
  useEffect(() => {
    if (mode === "setup") setPicked(chosenTypes ?? DEFAULT_TYPES);
    else if (mode === "idle") setPicked([]);
  }, [mode, chosenTypes]);

  const [survey, setSurvey] = useState<Survey | "counting" | "failed" | null>(null);
  const count = async (includeProtected: boolean, quietly = false) => {
    if (!quietly) setSurvey("counting");
    try {
      setSurvey(await api.surveyUserFiles(includeProtected));
    } catch {
      if (!quietly) setSurvey("failed");
    }
  };
  useEffect(() => {
    if (view === "setup" && survey === null) void count(false);
  }, [view, survey]);
  const surveyNow = useLatest(survey);
  useEffect(() => {
    if (active && view === "setup" && isCounted(surveyNow.current)) void count(false, true);
  }, [active, view, surveyNow]);
  const counted = isCounted(survey) ? survey : null;
  const [estimate, setEstimate] = useState<Estimate | null>(null);
  useEffect(() => {
    if (!counted) return;
    let gone = false;
    let timer = 0;
    const ask = async () => {
      try {
        const next = await api.indexEstimate();
        if (gone) return;
        setEstimate(next);
        if (next.measuring) timer = window.setTimeout(ask, ESTIMATE_POLL_MS);
      } catch {
      }
    };
    void ask();
    return () => {
      gone = true;
      window.clearTimeout(timer);
    };
  }, [counted]);

  useEffect(() => {
    if (running) setStarting(false);
  }, [running]);

  const [stopAsked, setStopAsked] = useState(false);
  const [stopping, setStopping] = useState(false);
  const stop = async () => {
    setStopAsked(false);
    setStopping(true);
    setError(null);
    try {
      await api.stopIndexing();
      await refresh();
    } catch (e) {
      setError(errorText(e));
    } finally {
      setStopping(false);
    }
  };

  const [removeAsked, setRemoveAsked] = useState(false);
  const [removing, setRemoving] = useState(false);
  useTauriEvent(MAIN_HIDDEN, () => setRemoveAsked(false));
  const removeIndex = async () => {
    setRemoveAsked(false);
    setRemoving(true);
    setError(null);
    try {
      const { indexed: roots } = await api.indexLists();
      await api.removeFromIndex(roots.map((root) => root.path));
      await refresh();
    } catch (e) {
      setError(errorText(e));
    } finally {
      setRemoving(false);
    }
  };

  const toggle = (type: Category) =>
    setPicked((now) => (now.includes(type) ? now.filter((t) => t !== type) : [...now, type]));

  const start = async () => {
    setStarting(true);
    setError(null);
    try {
      if (mode === "setup") {
        await indexed.set(picked);
        await api.indexEverything();
      } else {
        await indexed.set(typesToIndex(indexedTypes, picked, false));
      }
      await refresh();
    } catch (e) {
      setError(errorText(e));
    } finally {
      setStarting(false);
    }
  };

  const total = useMemo(
    () => (survey === "counting" ? COUNTING_TOTAL : counted ? (picked.length > 0 ? indexTotal(counted, estimate, picked) : NO_TOTAL) : null),
    [survey, counted, estimate, picked],
  );

  const everyType = TYPE_ORDER.every((type) => indexedTypes.includes(type));
  const checked = mode === "idle" ? typesToIndex(indexedTypes, picked, false) : picked;
  const canStart = mode === "setup" ? picked.length > 0 : hasNewTypes(indexedTypes, picked);
  const named = active ? { viewTransitionName: SWITCH_NAME } : undefined;

  return (
    <div className={styles.page}>
      <header className={styles.bar}>
        <IndexHeader onBack={onBack} />
      </header>

      <div className={styles.scroll}>
        <div className={styles.content}>
          <section className={styles.explainer}>
            <p className={styles.lead}>Files can only be searched for after they are indexed.</p>
            <p>This can take a while, so choose only what you want to search.</p>
          </section>

          <div className={styles.stage}>
          <div className={styles.cardsAndNote}>
            <section className={styles.card} aria-busy={!ready}>
              {ready && view === "running" && progress && (
                <>
                  <h2 className={styles.cardTitle}>{progress.updating ? "Updating your index" : "Indexing your files"}</h2>
                  <div className={styles.middle}>
                    <SimpleProgress progress={progress} secondsLeft={secondsLeft} onScreen={active} onRefresh={refresh} onError={setError} />
                    {hint && (
                      <p className={styles.hint}>
                        <Info size={16} aria-hidden />
                        <span>
                          You can close this window; indexing keeps going in the background.{" "}
                          <button type="button" className={styles.hintLink} onClick={onSearch}>
                            Search what&rsquo;s indexed so far
                            <Search size={13} aria-hidden />
                          </button>
                        </span>
                      </p>
                    )}
                  </div>
                  <div className={styles.foot}>
                    <div className={styles.footActions}>
                      {!progress.updating && (
                        <Button variant="dangerOutline" onClick={() => setStopAsked(true)} disabled={stopping}>
                          Stop indexing
                        </Button>
                      )}
                      <Button variant="secondary" onClick={onAdvanced} style={named}>
                        See details in advanced view
                      </Button>
                    </div>
                  </div>
                </>
              )}

              {ready && view === "setup" && (
                <>
                  <h2 className={styles.cardTitle}>Index my files</h2>
                  <p className={styles.cardText}>Everything in your user folder, of these types:</p>
                  <TypeTiles checked={checked} locked={[]} disabled={starting} onToggle={toggle} />
                  <div className={styles.actions}>
                    {counted && counted.notAsked.length > 0 && <AccessNote onAllow={() => void count(true)} />}
                    {total && <TotalSwap total={total} />}
                    <Button variant="primary" onClick={start} disabled={!canStart || starting}>
                      <DatabaseZap size={16} aria-hidden />
                      {starting ? "Starting…" : "Index my files"}
                    </Button>
                  </div>
                </>
              )}

              {ready && view === "idle" && (
                <>
                  <h2 className={styles.cardTitle}>Your index</h2>
                  <p className={styles.cardText}>
                    {everyType ? "Every type of file is indexed." : `${indexedSentence(indexedTypes)} Tick more types to add them.`}
                  </p>
                  <TypeTiles checked={checked} locked={indexedTypes} disabled={starting} onToggle={toggle} />
                  <div className={cx(styles.actions, styles.buttonPair)}>
                    <Button variant="dangerOutline" onClick={() => setRemoveAsked(true)} disabled={starting || removing}>
                      <Trash2 size={16} aria-hidden />
                      {removing ? "Removing…" : "Remove index"}
                    </Button>
                    <Button
                      variant="primary"
                      onClick={start}
                      disabled={!canStart || starting}
                      data-tooltip={everyType ? "All files are already indexed." : undefined}
                    >
                      <DatabaseZap size={16} aria-hidden />
                      {starting ? "Starting…" : "Add to index"}
                    </Button>
                  </div>
                </>
              )}

              {error && (
                <p className={styles.error} role="alert">
                  {error}
                </p>
              )}
            </section>

            {ready && (
              <div className={cx(styles.advanced, view === "running" && styles.advancedAway)} inert={view === "running"}>
                <p>Use the advanced view to choose folders or exclude items.</p>
                <Button variant="secondary" onClick={onAdvanced} style={view === "running" ? undefined : named}>
                  Advanced view
                </Button>
              </div>
            )}
          {ready && view === "setup" && counted && counted.noAccess.length > 0 && (
            <p className={styles.deniedNote}>
              Mesa isn&rsquo;t allowed to look in {counted.noAccess.length === 1 ? "this folder" : "these folders"}:{" "}
              {joinNames(counted.noAccess)}.
              <br />
              To allow access, double-click {counted.noAccess.length === 1 ? "it" : "them"} in the advanced view, or open System Settings &rsaquo;
              Privacy &amp; Security.
            </p>
          )}
          </div>
          </div>
        </div>
      </div>

      <ConfirmDialog
        open={stopAsked}
        title="Stop indexing?"
        confirmLabel="Stop indexing"
        danger
        onCancel={() => setStopAsked(false)}
        onConfirm={stop}
      >
        <p>This will stop indexing and remove what has been indexed so&nbsp;far. Your files themselves are not touched.</p>
      </ConfirmDialog>

      <ConfirmDialog
        open={removeAsked}
        title="Remove the index?"
        confirmLabel="Remove index"
        danger
        onCancel={() => setRemoveAsked(false)}
        onConfirm={removeIndex}
      >
        <p>
          This will remove all {fileCount.format(overview?.files ?? 0)} {overview?.files === 1 ? "file" : "files"} from the index. Your files
          themselves are not touched.
        </p>
        <p>The types you chose and the items you excluded are kept, for if you index again later.</p>
      </ConfirmDialog>
    </div>
  );
}

function indexedSentence(types: Category[]): string {
  const words = describeTypes(types);
  const singular = types.length === 1 && (types[0] === "audio" || types[0] === "code");
  return `${words.charAt(0).toUpperCase()}${words.slice(1)} ${singular ? "is" : "are"} indexed.`;
}

function isCounted(survey: Survey | "counting" | "failed" | null): survey is Survey {
  return typeof survey === "object" && survey !== null;
}

const fileCount = new Intl.NumberFormat();
const ESTIMATE_POLL_MS = 1000;

const COUNTING_TOTAL: Total = {
  id: "counting",
  counted: false,
  node: (
    <span className={styles.total} data-counting="">
      Counting files…
    </span>
  ),
};

const NO_TOTAL: Total = { id: "none", counted: true, node: null };

interface Total {
  id: string;
  counted: boolean;
  node: ReactNode;
}

function indexTotal(survey: Survey, estimate: Estimate | null, types: Category[]): Total {
  const files = describeTotal(survey, types);
  const fast = estimate ? estimateSeconds(estimate, types) : null;
  if (estimate === null || fast === null) {
    const measuring = estimate === null || estimate.measuring;
    return {
      id: `${files}|${measuring}`,
      counted: true,
      node: (
        <span className={styles.total}>
          {files}
          {measuring && <span data-counting="">, estimating…</span>}
        </span>
      ),
    };
  }
  const time = formatEstimateRange(fast, fast * estimate.gentleSlowdown, survey.notAsked.length > 0);
  return {
    id: `${files}|${time}`,
    counted: true,
    node: (
      <span className={styles.total} data-tooltip="With fast indexing, or without (the default).">
        {files}, {time}
      </span>
    ),
  };
}

const OUT_MS = 200;

function TotalSwap({ total }: { total: Total }) {
  const [shown, setShown] = useState({ ...total, enter: "fade" as "fade" | "slide" });
  const [leaving, setLeaving] = useState(false);
  if (total.id === shown.id && total.node !== shown.node) setShown({ ...shown, node: total.node });
  const latest = useLatest(total);
  useEffect(() => {
    if (total.id === shown.id) return;
    const enter = shown.counted && total.counted ? "slide" : "fade";
    if (prefersReducedMotion()) {
      setShown({ ...latest.current, enter });
      return;
    }
    setLeaving(true);
    const timer = window.setTimeout(() => {
      setShown({ ...latest.current, enter });
      setLeaving(false);
    }, OUT_MS);
    return () => window.clearTimeout(timer);
  }, [total.id, total.counted, shown, latest]);
  return (
    <span
      key={shown.id}
      className={cx(styles.swap, shown.enter === "slide" ? styles.slideIn : styles.fadeIn, leaving && styles.leaving)}
    >
      {shown.node}
    </span>
  );
}

function AccessNote({ onAllow }: { onAllow: () => void }) {
  return (
    <p className={styles.accessNote}>
      Mesa needs access to your personal files in order to index and search them.{" "}
      <button type="button" className={styles.hintLink} onClick={onAllow}>
        Allow access
      </button>
    </p>
  );
}

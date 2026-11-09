import { useEffect, useState } from "react";
import { ArrowUpRight, DatabaseZap, FolderPlus, FolderSearch, Info, Lock, Search } from "lucide-react";
import { api, errorText, type Category, type Overview } from "../api";
import { Button } from "../components/Button";
import { ConfirmDialog } from "../components/ConfirmDialog";
import { IndexHeader } from "../components/IndexHeader";
import { SimpleProgress } from "../components/SimpleProgress";
import { TypeTiles } from "../components/TypeTiles";
import { DEFAULT_TYPES, TYPE_ORDER, describeTypes, hasNewTypes, typesToIndex } from "../indexTypes";
import { HINT_DELAY_MS, hasShownBackgroundHint, markBackgroundHintShown } from "../backgroundHint";
import { removeItems } from "../confirmations";
import { SWITCH_NAME } from "../indexView";
import { changeWithTransition } from "../viewTransition";
import { useIndexProgress } from "../useIndexProgress";
import { useIndexedTypes } from "../useIndexedTypes";
import styles from "./SimpleIndexScreen.module.css";

type Mode = "setup" | "running" | "idle";
const USE_ICON = 16;
const ADVANCED_USES = [
  { Icon: FolderSearch, text: "Browse and see what's indexed", size: USE_ICON },
  { Icon: Lock, text: "Choose which items to exclude", size: 15 },
  { Icon: FolderPlus, text: "Index single folders or files", size: USE_ICON },
];

interface Props {
  onBack: () => void;
  onAdvanced: () => void;
  onSearch: () => void;
  overview: Overview | null;
  secondsLeft: number | null;
  onRefreshOverview: () => Promise<void>;
  active: boolean;
}

export function SimpleIndexScreen({ onBack, onAdvanced, onSearch, overview, secondsLeft, onRefreshOverview, active }: Props) {
  const { progress: polled, refresh: refreshProgress } = useIndexProgress(active);
  const [heard, setHeard] = useState(false);
  useEffect(() => {
    if (!active) setHeard(false);
  }, [active]);
  useEffect(() => {
    if (active) setHeard(true);
  }, [polled]); // eslint-disable-line react-hooks/exhaustive-deps -- only a new answer is news, not the page being come to
  const progress = heard ? polled : (overview?.progress ?? null);
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
  const [animated, setAnimated] = useState(false);
  useEffect(() => {
    if (!settled) return;
    let frame = requestAnimationFrame(() => {
      frame = requestAnimationFrame(() => setAnimated(true));
    });
    return () => cancelAnimationFrame(frame);
  }, [settled]);

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

  useEffect(() => {
    if (mode === "setup") setPicked(DEFAULT_TYPES);
    else if (mode === "idle") setPicked([]);
  }, [mode]);

  useEffect(() => {
    if (running) setStarting(false);
  }, [running]);

  const [stopAsked, setStopAsked] = useState(false);
  const [stopping, setStopping] = useState(false);
  const [queuedRoots, setQueuedRoots] = useState<string[]>([]);
  const queueKey = view === "running" && progress ? `${progress.name}|${progress.queued}` : "";
  useEffect(() => {
    if (!queueKey || !active) return;
    let cancelled = false;
    api
      .indexedItems()
      .then((items) => {
        if (!cancelled) setQueuedRoots(items.filter((item) => item.droppable).map((item) => item.path));
      })
      .catch(() => {});
    return () => {
      cancelled = true;
    };
  }, [queueKey, active]);
  const stop = async () => {
    setStopAsked(false);
    setStopping(true);
    setError(null);
    try {
      const dropping =
        queuedRoots.length > 0
          ? queuedRoots
          : (await api.indexedItems()).filter((item) => item.droppable).map((item) => item.path);
      await removeItems(dropping, dropping);
      await Promise.all([refreshProgress(), onRefreshOverview().catch(() => {})]);
    } catch (e) {
      setError(errorText(e));
    } finally {
      setStopping(false);
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
      await Promise.all([refreshProgress(), onRefreshOverview().catch(() => {})]);
    } catch (e) {
      setError(errorText(e));
    } finally {
      setStarting(false);
    }
  };

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
            <p className={styles.lead}>Files need to be indexed before they can be found via search.</p>
            <p>This process can take a while, so only index the files you care about searching.</p>
          </section>

          <div className={styles.stage}>
          <div
            className={styles.choices}
            data-running={view === "running" ? "" : undefined}
            data-animated={animated && active ? "" : undefined}
          >
            <section className={styles.card} aria-busy={!ready}>
              {ready && view === "running" && progress && (
                <>
                  <h2 className={styles.cardTitle}>{progress.updating ? "Updating your index" : "Indexing your files"}</h2>
                  <div className={styles.middle}>
                    <SimpleProgress progress={progress} secondsLeft={secondsLeft} onScreen={active} onRefresh={refreshProgress} onError={setError} />
                    {hint && (
                      <p className={styles.hint}>
                        <Info size={16} aria-hidden />
                        <span>
                          You can close this window. Indexing keeps going in the background.{" "}
                          <button type="button" className={styles.hintLink} onClick={onSearch}>
                            Search what&rsquo;s indexed so far
                            <Search size={13} aria-hidden />
                          </button>
                        </span>
                      </p>
                    )}
                  </div>
                  <div className={styles.foot}>
                    <p className={styles.note}>Indexing {describeTypes(indexedTypes)}.</p>
                    <div className={styles.footActions}>
                      {!progress.updating && (
                        <Button variant="dangerOutline" onClick={() => setStopAsked(true)} disabled={stopping}>
                          Stop
                        </Button>
                      )}
                      <Button variant="secondary" onClick={onAdvanced} style={named}>
                        See details in the advanced view
                        <ArrowUpRight size={16} aria-hidden />
                      </Button>
                    </div>
                  </div>
                </>
              )}

              {ready && view === "setup" && (
                <>
                  <h2 className={styles.cardTitle}>Index my files</h2>
                  <p className={styles.cardText}>Everything in your user folder, of the types:</p>
                  <TypeTiles checked={checked} locked={[]} disabled={starting} onToggle={toggle} />
                  <div className={styles.actions}>
                    <Button variant="primary" onClick={start} disabled={!canStart || starting}>
                      <DatabaseZap size={16} aria-hidden />
                      {starting ? "Starting…" : "Index my files"}
                    </Button>
                  </div>
                </>
              )}

              {ready && view === "idle" && (
                <>
                  <h2 className={styles.cardTitle}>Your files are indexed</h2>
                  <p className={styles.cardText}>{everyType ? "Every type is indexed." : "Add more types:"}</p>
                  <TypeTiles checked={checked} locked={indexedTypes} disabled={starting} onToggle={toggle} />
                  {!everyType && (
                    <div className={styles.actions}>
                      <Button variant="primary" onClick={start} disabled={!canStart || starting}>
                        <DatabaseZap size={16} aria-hidden />
                        {starting ? "Starting…" : "Index these too"}
                      </Button>
                    </div>
                  )}
                </>
              )}

              {error && (
                <p className={styles.error} role="alert">
                  {error}
                </p>
              )}
            </section>

            <div className={styles.or} aria-hidden>
              <span className={styles.rule} />
              <span className={styles.orWord}>or</span>
              <span className={styles.rule} />
            </div>

            <div className={styles.other} inert={view === "running"}>
              <section className={styles.otherCard}>
                <h2 className={styles.cardTitle}>Choose folders yourself</h2>
                <p className={styles.cardText}>Pick exactly what to index and what to leave out.</p>
                <ul className={styles.uses}>
                  {ADVANCED_USES.map(({ Icon, text, size }) => (
                    <li key={text}>
                      <span className={styles.useIcon}>
                        <Icon size={size} aria-hidden />
                      </span>
                      {text}
                    </li>
                  ))}
                </ul>
                <div className={styles.actions}>
                  <Button variant="secondary" onClick={onAdvanced} style={view === "running" ? undefined : named}>
                    Advanced view
                    <ArrowUpRight size={16} aria-hidden />
                  </Button>
                </div>
              </section>
            </div>
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
    </div>
  );
}

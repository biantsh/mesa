import { useEffect, useLayoutEffect, useRef, useState, type ReactNode } from "react";
import { Pause, Play } from "lucide-react";
import type { Progress } from "../api";
import { cx } from "../cx";
import { formatEta } from "../eta";
import { useIndexControls } from "../useIndexControls";
import { IconButton } from "./IconButton";
import { Reveal } from "./Reveal";
import { SpeedButton } from "./SpeedButton";
import styles from "./IndexProgress.module.css";

const number = new Intl.NumberFormat();
const STOP_WINDOW_MS = 3000;

interface Props {
  progress: Progress | null;
  secondsLeft: number | null;
  onRefresh: () => Promise<void>;
  onError: (message: string) => void;
  stoppedAt: number;
  active: boolean;
}

interface Leaving {
  progress: Progress;
  completed: boolean;
}

function useLeaving(progress: Progress | null, stoppedAt: number) {
  const [previous, setPrevious] = useState(progress);
  const [leaving, setLeaving] = useState<Leaving | null>(null);
  if (progress !== previous) {
    setPrevious(progress);
    if (progress) setLeaving(null);
    else if (previous) {
      setLeaving({ progress: previous, completed: Date.now() - stoppedAt > STOP_WINDOW_MS });
    }
  }
  return { leaving, finish: () => setLeaving(null) };
}

export function IndexProgress({ progress: current, secondsLeft, onRefresh, onError, stoppedAt, active }: Props) {
  const { leaving, finish } = useLeaving(current, stoppedAt);
  const isLeaving = current === null && leaving !== null;

  const progress =
    current ??
    (leaving?.completed
      ? { ...leaving.progress, counting: false, paused: false, updating: false, done: leaving.progress.total }
      : (leaving?.progress ?? null));
  if (!progress) return null;

  return (
    <div className={styles.root}>
      <Reveal show={!isLeaving} from="right" hold={leaving?.completed} onExited={finish}>
        <Row progress={progress} secondsLeft={secondsLeft} onScreen={active && !isLeaving} onRefresh={onRefresh} onError={onError} />
      </Reveal>
    </div>
  );
}

function Row({
  progress,
  secondsLeft,
  onScreen,
  onRefresh,
  onError,
}: {
  progress: Progress;
  secondsLeft: number | null;
  onScreen: boolean;
  onRefresh: () => Promise<void>;
  onError: (message: string) => void;
}) {
  const { togglePause, toggleTurbo } = useIndexControls(progress, onRefresh, onError);

  return (
    <div className={styles.row} role="group" aria-label="Indexing progress">
      <Label progress={progress} />
      <Bar progress={progress} />
      <Count progress={progress} eta={formatEta(secondsLeft)} />
      <span className={styles.buttons}>
        <SpeedButton progress={progress} secondsLeft={secondsLeft} onScreen={onScreen} onToggle={toggleTurbo} />
        <IconButton label={progress.paused ? "Resume indexing" : "Pause indexing"} onClick={togglePause}>
          {progress.paused ? <Play size={18} aria-hidden /> : <Pause size={18} aria-hidden />}
        </IconButton>
      </span>
    </div>
  );
}

export function whereIsIndexing({ counting, folders }: Progress): string | null {
  return !counting && folders.length > 0 ? folders.join(" › ") : null;
}

const quoted = (name: string) => `\u201c${name}\u201d`;

export function Label({ progress }: { progress: Progress }) {
  const { name, queued, updating } = progress;
  const where = whereIsIndexing(progress);
  return (
    <span
      className={styles.label}
      data-tooltip={`${updating ? "Updating" : "Indexing"} ${quoted(name)}${queued > 0 ? ` + ${queued}` : ""}${where ? `\nin ${where}` : ""}`}
    >
      <span className={styles.name}>{name}</span>
      {queued > 0 && <span className={styles.fixed}>{`\u00a0+\u00a0${queued}`}</span>}
    </span>
  );
}

function Count({ progress, eta }: { progress: Progress; eta: string | null }) {
  const text = figures(progress);
  const tag = progress.paused ? "Paused" : eta;
  const [shown, setShown] = useState(tag);
  if (tag && tag !== shown) setShown(tag);
  return (
    <span className={styles.count}>
      <Reveal show={tag !== null}>
        <span className={styles.pausedTag}>
          <SmoothWidth quick>{shown}</SmoothWidth>
          {text && <span className={styles.bullet} aria-hidden />}
        </span>
      </Reveal>
      <SmoothWidth>
        <span key={progress.counting ? "finding" : "counted"} className={styles.text}>
          {text}
        </span>
      </SmoothWidth>
    </span>
  );
}

function SmoothWidth({ children, quick = false }: { children: ReactNode; quick?: boolean }) {
  const inner = useRef<HTMLSpanElement>(null);
  const [width, setWidth] = useState<number | null>(null);
  useLayoutEffect(() => {
    const el = inner.current;
    if (!el) return;
    const measure = () => setWidth(el.getBoundingClientRect().width);
    measure();
    const observer = new ResizeObserver(measure);
    observer.observe(el);
    return () => observer.disconnect();
  }, []);
  return (
    <span className={cx(styles.smooth, quick && styles.smoothQuick)} style={width === null ? undefined : { width }}>
      <span ref={inner} className={styles.smoothInner}>
        {children}
      </span>
    </span>
  );
}

export function figures({ counting, found, done, total, paused }: Progress): string {
  if (!counting && total > 0) return `${number.format(done)} of ${number.format(total)}`;
  return counting && !paused ? `Finding files\u2026 ${number.format(found)}` : "";
}

const AFTER_SWEEP_MS = 2_000;

export function Bar({ progress }: { progress: Progress }) {
  const { counting, done, total, paused, turbo } = progress;
  const determinate = !counting && total > 0;
  const percent = determinate ? Math.floor((done / total) * 100) : 0;
  const sweeping = !determinate && !paused;
  const [swept, setSwept] = useState(sweeping);
  const [settled, setSettled] = useState(false);
  if (sweeping && (!swept || settled)) {
    setSwept(true);
    setSettled(false);
  }
  const waiting = swept && !settled;
  useEffect(() => {
    if (!waiting || !determinate) return;
    const timer = window.setTimeout(() => setSettled(true), AFTER_SWEEP_MS);
    return () => window.clearTimeout(timer);
  }, [waiting, determinate]);

  return (
    <div
      className={cx(styles.track, paused && styles.paused, turbo && styles.turbo)}
      role="progressbar"
      aria-label="Indexing progress"
      aria-valuemin={0}
      aria-valuemax={determinate ? total : undefined}
      aria-valuenow={determinate ? done : undefined}
      aria-valuetext={determinate ? `${percent}%, ${done} of ${total} files` : "Finding files"}
      data-tooltip={determinate ? `${percent}% (${number.format(done)} of ${number.format(total)} files)` : undefined}
    >
      <div
        className={cx(styles.fill, determinate && done > 0 && styles.started, waiting && styles.afterSweep)}
        style={{ width: determinate ? `${percent}%` : "0%" }}
      />
      <div className={cx(styles.searching, (determinate || paused) && styles.searchingGone)} />
    </div>
  );
}

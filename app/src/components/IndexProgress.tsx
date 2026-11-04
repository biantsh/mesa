import { useState } from "react";
import { Pause, Play, X } from "lucide-react";
import { api, errorText, type Progress } from "../api";
import { cx } from "../cx";
import { IconButton } from "./IconButton";
import { Reveal } from "./Reveal";
import styles from "./IndexProgress.module.css";

const number = new Intl.NumberFormat();
const STOP_WINDOW_MS = 3000;

interface Props {
  progress: Progress | null;
  onRefresh: () => Promise<void>;
  onCancel: (subject: string) => void;
  onError: (message: string) => void;
  stoppedAt: number;
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

export function IndexProgress({ progress: current, onRefresh, onCancel, onError, stoppedAt }: Props) {
  const { leaving, finish } = useLeaving(current, stoppedAt);
  const isLeaving = current === null && leaving !== null;

  const progress =
    current ??
    (leaving?.completed
      ? { ...leaving.progress, counting: false, paused: false, updating: false, done: leaving.progress.total }
      : (leaving?.progress ?? null));
  if (!progress) return null;

  const togglePause = async () => {
    try {
      await api.setIndexingPaused(!progress.paused);
    } catch (e) {
      onError(`Couldn't ${progress.paused ? "resume" : "pause"} indexing. ${errorText(e)}`);
    }
    onRefresh();
  };

  const stopUpdating = async () => {
    try {
      await api.cancelIndexing();
    } catch (e) {
      onError(`Couldn't stop updating. ${errorText(e)}`);
    }
    onRefresh();
  };

  return (
    <div className={styles.root}>
      <Reveal show={!isLeaving} from="right" hold={leaving?.completed} onExited={finish}>
        <div className={styles.row} role="group" aria-label="Indexing progress">
          <Label progress={progress} />
          <Bar progress={progress} />
          <Count progress={progress} />
          <IconButton
            label={progress.paused ? "Resume indexing" : "Pause indexing"}
            onClick={togglePause}
          >
            {progress.paused ? <Play size={18} aria-hidden /> : <Pause size={18} aria-hidden />}
          </IconButton>
          <IconButton
            label={progress.updating ? "Stop updating" : "Cancel indexing"}
            tone={progress.updating ? undefined : "danger"}
            onClick={progress.updating ? stopUpdating : () => onCancel(cancelSubject(progress))}
          >
            <X size={18} aria-hidden />
          </IconButton>
        </div>
      </Reveal>
    </div>
  );
}

const quoted = (name: string) => `\u201c${name}\u201d`;

function cancelSubject({ name, queued }: Progress): string {
  return queued > 0 ? `${quoted(name)} and ${queued} more` : quoted(name);
}

export function Label({ progress: { name, queued, updating } }: { progress: Progress }) {
  return (
    <span
      className={styles.label}
      title={`${updating ? "Updating" : "Indexing"} ${quoted(name)}${queued > 0 ? ` + ${queued}` : ""}`}
    >
      <span className={styles.name}>{name}</span>
      {queued > 0 && <span className={styles.fixed}>{`\u00a0+\u00a0${queued}`}</span>}
    </span>
  );
}

function Count({ progress }: { progress: Progress }) {
  const text = figures(progress);
  const longest = figures({ ...progress, done: progress.total });
  return (
    <span className={styles.count}>
      <span className={styles.sizer} aria-hidden>
        {longest}
      </span>
      <span className={styles.shown}>
        <Reveal show={progress.paused}>
          <span className={styles.pausedTag}>{text ? "Paused \u00b7" : "Paused"}</span>
        </Reveal>
        <span>{text}</span>
      </span>
    </span>
  );
}

export function figures({ counting, found, done, total, paused }: Progress): string {
  if (!counting && total > 0) return `${number.format(done)} of ${number.format(total)}`;
  return counting && !paused ? `Finding files\u2026 ${number.format(found)}` : "";
}

export function Bar({ progress }: { progress: Progress }) {
  const { counting, done, total, paused } = progress;
  const determinate = !counting && total > 0;
  const percent = determinate ? Math.floor((done / total) * 100) : 0;

  return (
    <div
      className={cx(styles.track, !determinate && !paused && styles.indeterminate, paused && styles.paused)}
      role="progressbar"
      aria-label="Indexing progress"
      aria-valuemin={0}
      aria-valuemax={determinate ? total : undefined}
      aria-valuenow={determinate ? done : undefined}
      aria-valuetext={determinate ? `${percent}%, ${done} of ${total} files` : "Finding files"}
      title={determinate ? `${percent}% \u00b7 ${number.format(done)} of ${number.format(total)} files` : undefined}
    >
      <div className={styles.fill} style={determinate ? { width: `${percent}%` } : undefined} />
    </div>
  );
}

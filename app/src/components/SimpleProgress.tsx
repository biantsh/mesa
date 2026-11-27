import { useState } from "react";
import { Pause, Play } from "lucide-react";
import type { Progress } from "../api";
import { formatEta } from "../eta";
import { useIndexControls } from "../useIndexControls";
import { IconButton } from "./IconButton";
import { Bar, Label, LOOKING_DEEPER, figures, whereIsIndexing } from "./IndexProgress";
import { SpeedButton } from "./SpeedButton";
import styles from "./SimpleProgress.module.css";

interface Props {
  progress: Progress;
  secondsLeft: number | null;
  onScreen: boolean;
  onRefresh: () => Promise<void>;
  onError: (message: string) => void;
}

export function SimpleProgress({ progress, secondsLeft, onScreen, onRefresh, onError }: Props) {
  const { togglePause, toggleTurbo } = useIndexControls(progress, onRefresh, onError);
  const left = progress.paused ? "Paused" : formatEta(secondsLeft, true);
  const where = whereIsIndexing(progress);
  const kind = progress.paused ? "paused" : left ? "left" : "none";
  const [shown, setShown] = useState(kind);
  const [moves, setMoves] = useState(false);
  if (shown !== kind) {
    setShown(kind);
    setMoves(true);
  }

  return (
    <div className={styles.root} role="group" aria-label="Indexing progress">
      <div className={styles.head}>
        <div className={styles.what}>
          {progress.refining ? <span className={styles.about}>{LOOKING_DEEPER}</span> : <Label progress={progress} />}
          {where && <span className={styles.where}>{where}</span>}
        </div>
        <span className={styles.buttons}>
          <SpeedButton progress={progress} secondsLeft={secondsLeft} onScreen={onScreen} onToggle={toggleTurbo} />
          <IconButton label={progress.paused ? "Resume indexing" : "Pause indexing"} onClick={togglePause}>
            {progress.paused ? <Play size={18} aria-hidden /> : <Pause size={18} aria-hidden />}
          </IconButton>
        </span>
      </div>
      <div className={styles.bar}>
        <Bar progress={progress} />
      </div>
      <div className={styles.foot}>
        <span key={kind} className={moves ? styles.comes : undefined}>
          {left}
        </span>
        <span>{figures(progress)}</span>
      </div>
    </div>
  );
}

import { ChevronLeft, DatabaseZap } from "lucide-react";
import type { Progress } from "../api";
import { SELECTING_FROM } from "../selection";
import { IconButton } from "./IconButton";
import { MorphButton } from "./MorphButton";
import { IndexProgress } from "./IndexProgress";
import styles from "./TopBar.module.css";

interface SelectionActions {
  selected: number;
  add: number;
  onAdd: () => void;
}

interface Props {
  onBack: () => void;
  progress: Progress | null;
  onRefreshProgress: () => Promise<void>;
  onIndexAll: () => void;
  everythingIndexed: boolean;
  onError: (message: string) => void;
  stoppedAt: number;
  summary: string | null;
  selection: SelectionActions;
}

function plural(count: number): string {
  return count === 1 ? "item" : "items";
}

export function TopBar({
  onBack,
  progress,
  onRefreshProgress,
  onIndexAll,
  everythingIndexed,
  onError,
  stoppedAt,
  summary,
  selection,
}: Props) {
  const selecting = selection.selected >= SELECTING_FROM;
  return (
    <header className={styles.bar} data-busy={progress ? "" : undefined}>
      <div className={styles.heading}>
        <IconButton label="Back to search" onClick={onBack}>
          <ChevronLeft size={24} absoluteStrokeWidth strokeWidth={1.67} aria-hidden />
        </IconButton>
        <div className={styles.titleGroup}>
          <h1 className={styles.title}>Index</h1>
          {summary && <span className={styles.summary}>{summary}</span>}
        </div>
      </div>
      <div className={styles.middle}>
        <IndexProgress
          progress={progress}
          onRefresh={onRefreshProgress}
          onError={onError}
          stoppedAt={stoppedAt}
        />
      </div>
      <MorphButton
        variant="primary"
        className={styles.indexButton}
        selecting={selecting}
        icon={<DatabaseZap size={15} />}
        iconSize={15}
        label="Index all"
        selectingLabel="Index selected"
        onClick={selecting ? selection.onAdd : onIndexAll}
        disabled={selecting ? selection.add === 0 : everythingIndexed}
        data-tooltip={
          !selecting
            ? everythingIndexed
              ? progress
                ? "Already indexing everything"
                : "Everything is already indexed"
              : undefined
            : selection.add === 0
              ? "Nothing selected can be indexed"
              : `Index ${selection.add} selected ${plural(selection.add)}`
        }
      />
    </header>
  );
}

import { ChevronLeft, DatabaseZap, Eraser } from "lucide-react";
import type { Progress } from "../api";
import { IconButton } from "./IconButton";
import { MorphButton } from "./MorphButton";
import { IndexProgress } from "./IndexProgress";
import styles from "./TopBar.module.css";

interface SelectionActions {
  selected: number;
  add: number;
  remove: number;
  onAdd: () => void;
  onRemove: () => void;
}

interface Props {
  onBack: () => void;
  progress: Progress | null;
  onRefreshProgress: () => Promise<void>;
  onIndexAll: () => void;
  onClearIndex: () => void;
  onCancelIndexing: (subject: string) => void;
  onError: (message: string) => void;
  stoppedAt: number;
  selection: SelectionActions;
}

const SELECTING_FROM = 2;

function plural(count: number): string {
  return count === 1 ? "item" : "items";
}

export function TopBar({
  onBack,
  progress,
  onRefreshProgress,
  onIndexAll,
  onClearIndex,
  onCancelIndexing,
  onError,
  stoppedAt,
  selection,
}: Props) {
  const selecting = selection.selected >= SELECTING_FROM;
  return (
    <header className={styles.bar}>
      <div className={styles.heading}>
        <IconButton label="Back to search" onClick={onBack}>
          <ChevronLeft size={24} absoluteStrokeWidth strokeWidth={1.67} aria-hidden />
        </IconButton>
        <h1 className={styles.title}>Index</h1>
      </div>
      <div className={styles.middle}>
        <IndexProgress
          progress={progress}
          onRefresh={onRefreshProgress}
          onCancel={onCancelIndexing}
          onError={onError}
          stoppedAt={stoppedAt}
        />
      </div>
      <MorphButton
        variant="primary"
        selecting={selecting}
        icon={<DatabaseZap size={16} />}
        label="Index my files"
        selectingLabel="Index selected"
        onClick={selecting ? selection.onAdd : onIndexAll}
        disabled={selecting && selection.add === 0}
        title={
          !selecting
            ? undefined
            : selection.add === 0
              ? "Nothing selected can be indexed"
              : `Index ${selection.add} selected ${plural(selection.add)}`
        }
      />
      <MorphButton
        variant="dangerOutline"
        selecting={selecting}
        icon={<Eraser size={16} />}
        label="Clear index"
        selectingLabel="Remove selected"
        onClick={selecting ? selection.onRemove : onClearIndex}
        disabled={selecting && selection.remove === 0}
        title={
          !selecting
            ? undefined
            : selection.remove === 0
              ? "Nothing selected is in the index"
              : `Remove ${selection.remove} selected ${plural(selection.remove)} from the index`
        }
      />
    </header>
  );
}

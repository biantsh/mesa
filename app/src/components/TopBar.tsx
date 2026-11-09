import { ArrowDownLeft, DatabaseZap } from "lucide-react";
import type { Progress } from "../api";
import { SELECTING_FROM } from "../selection";
import { SWITCH_NAME } from "../indexView";
import { Button } from "./Button";
import { IndexHeader } from "./IndexHeader";
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
  secondsLeft: number | null;
  onRefreshProgress: () => Promise<void>;
  onIndexAll: () => void;
  everythingIndexed: boolean;
  onError: (message: string) => void;
  stoppedAt: number;
  selection: SelectionActions;
  active: boolean;
  onSimpleView: () => void;
}

function plural(count: number): string {
  return count === 1 ? "item" : "items";
}

export function TopBar({
  onBack,
  progress,
  secondsLeft,
  onRefreshProgress,
  onIndexAll,
  everythingIndexed,
  onError,
  stoppedAt,
  selection,
  active,
  onSimpleView,
}: Props) {
  const selecting = selection.selected >= SELECTING_FROM;
  return (
    <header className={styles.bar} data-busy={progress ? "" : undefined}>
      <IndexHeader onBack={onBack} />
      <div className={styles.middle}>
        <IndexProgress
          progress={progress}
          secondsLeft={secondsLeft}
          onRefresh={onRefreshProgress}
          onError={onError}
          stoppedAt={stoppedAt}
          active={active}
        />
      </div>
      <div className={styles.actions}>
        <Button
          variant="secondary"
          className={styles.simpleButton}
          onClick={onSimpleView}
          style={{ viewTransitionName: active ? SWITCH_NAME : undefined }}
        >
          <ArrowDownLeft size={15} aria-hidden />
          Simple view
        </Button>
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
      </div>
    </header>
  );
}

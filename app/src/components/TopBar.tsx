import { DatabaseZap } from "lucide-react";
import type { Category, Progress } from "../api";
import { NO_TYPES_TIP } from "../indexTypes";
import { SELECTING_FROM } from "../selection";
import { SWITCH_NAME } from "../indexView";
import { Button } from "./Button";
import { IndexHeader } from "./IndexHeader";
import { MorphButton } from "./MorphButton";
import { IndexProgress } from "./IndexProgress";
import { TypeToggles } from "./TypeToggles";
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
  types: Category[] | null;
  onToggleType: (type: Category) => void;
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
  types,
  onToggleType,
}: Props) {
  const selecting = selection.selected >= SELECTING_FROM;
  const noTypes = types?.length === 0;
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
        <TypeToggles types={types} onToggle={onToggleType} />
      </div>
      <div className={styles.actions}>
        <Button
          variant="secondary"
          className={styles.simpleButton}
          onClick={onSimpleView}
          style={{ viewTransitionName: active ? SWITCH_NAME : undefined }}
        >
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
          disabled={noTypes || (selecting ? selection.add === 0 : everythingIndexed)}
          data-tooltip={
            noTypes
              ? NO_TYPES_TIP
              : !selecting
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

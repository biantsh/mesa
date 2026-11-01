import { DatabaseZap, Eraser } from "lucide-react";
import { Button } from "./Button";
import { IndexProgress } from "./IndexProgress";
import styles from "./TopBar.module.css";

interface Props {
  onIndexAll: () => void;
  onClearIndex: () => void;
  onCancelIndexing: (subject: string) => void;
  onError: (message: string) => void;
  stoppedAt: number;
}

export function TopBar({ onIndexAll, onClearIndex, onCancelIndexing, onError, stoppedAt }: Props) {
  return (
    <header className={styles.bar}>
      <Button variant="primary" onClick={onIndexAll}>
        <DatabaseZap size={16} aria-hidden />
        Index all
      </Button>
      <div className={styles.middle}>
        <IndexProgress onCancel={onCancelIndexing} onError={onError} stoppedAt={stoppedAt} />
      </div>
      <Button variant="dangerOutline" onClick={onClearIndex}>
        <Eraser size={16} aria-hidden />
        Clear index
      </Button>
    </header>
  );
}

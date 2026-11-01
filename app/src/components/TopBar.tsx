import { DatabaseZap, Eraser } from "lucide-react";
import { Button } from "./Button";
import styles from "./TopBar.module.css";

interface Props {
  onIndexAll: () => void;
  onClearIndex: () => void;
}

export function TopBar({ onIndexAll, onClearIndex }: Props) {
  return (
    <header className={styles.bar}>
      <Button variant="primary" onClick={onIndexAll}>
        <DatabaseZap size={16} aria-hidden />
        Index all
      </Button>
      <Button variant="dangerOutline" onClick={onClearIndex}>
        <Eraser size={16} aria-hidden />
        Clear index
      </Button>
    </header>
  );
}

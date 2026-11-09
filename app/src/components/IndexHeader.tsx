import { ChevronLeft } from "lucide-react";
import { IconButton } from "./IconButton";
import styles from "./IndexHeader.module.css";

export function IndexHeader({ onBack }: { onBack: () => void }) {
  return (
    <div className={styles.heading}>
      <IconButton label="Back to search" onClick={onBack}>
        <ChevronLeft size={24} absoluteStrokeWidth strokeWidth={1.67} aria-hidden />
      </IconButton>
      <h1 className={styles.title}>Index</h1>
    </div>
  );
}

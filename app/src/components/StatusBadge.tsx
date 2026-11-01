import { CircleCheck, Lock, LoaderCircle } from "lucide-react";
import type { Entry } from "../api";
import styles from "./StatusBadge.module.css";

export function statusText(entry: Entry): string | null {
  if (!entry.readable) return "Can't be opened";
  if (entry.index === "indexed") return "Indexed";
  if (entry.index === "indexing") return "Indexing";
  return null;
}

export function StatusBadge({ entry }: { entry: Entry }) {
  const label = statusText(entry);
  if (!label) return null;

  if (!entry.readable) {
    return (
      <span className={`${styles.badge} ${styles.lock}`} role="img" aria-label={label}>
        <Lock size={12} strokeWidth={2.4} />
      </span>
    );
  }
  if (entry.index === "indexing") {
    return (
      <span className={`${styles.badge} ${styles.indexing}`} role="img" aria-label={label}>
        <LoaderCircle size={18} strokeWidth={2.4} />
      </span>
    );
  }
  return (
    <span className={`${styles.badge} ${styles.indexed}`} role="img" aria-label={label}>
      <CircleCheck size={20} />
    </span>
  );
}

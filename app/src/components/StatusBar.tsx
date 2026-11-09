import type { Entry } from "../api";
import { splitItems } from "../format";
import styles from "./StatusBar.module.css";

interface Props {
  entries: Entry[] | null;
  selected: Entry[];
  selectedInList: { name: string; count: number } | null;
}

export function StatusBar({ entries, selected, selectedInList }: Props) {
  const items = entries !== null ? splitItems(entries) : null;
  const inList = selectedInList !== null && selectedInList.count > 0;
  return (
    <footer className={styles.bar} data-overlay>
      {items !== null && (
        <span className={styles.folder}>
          <span>
            {items.total}
            {items.detail && <span className={styles.detail}> {items.detail}</span>}
          </span>
          <span className={styles.separator} aria-hidden />
          <span className={styles.selected}>
            {inList ? `${selectedInList.count} selected in ${selectedInList.name}` : `${selected.length} selected`}
          </span>
        </span>
      )}
    </footer>
  );
}

import type { Entry } from "../api";
import { describeIndexed, describeItems } from "../format";
import styles from "./StatusBar.module.css";

interface Props {
  entries: Entry[] | null;
  selected: Entry[];
  selectedInList: { name: string; count: number } | null;
}

export function StatusBar({ entries, selected, selectedInList }: Props) {
  if (entries === null) return <footer className={styles.bar} />;
  const indexed = selected.length > 0 ? describeIndexed(selected, true) : describeIndexed(entries);
  return (
    <footer className={styles.bar}>
      <span>{describeItems(entries)}</span>
      {selected.length > 0 && (
        <>
          <span className={styles.separator} aria-hidden />
          <span>{selected.length} selected</span>
        </>
      )}
      {selectedInList !== null && selectedInList.count > 0 && (
        <>
          <span className={styles.separator} aria-hidden />
          <span>
            {selectedInList.count} selected in {selectedInList.name}
          </span>
        </>
      )}
      {indexed && <span className={styles.indexed}>{indexed}</span>}
    </footer>
  );
}

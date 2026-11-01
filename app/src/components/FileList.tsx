import { memo, useRef, type Ref } from "react";
import { ChevronDown, ChevronUp } from "lucide-react";
import { previewUrl, type Entry } from "../api";
import { cx } from "../cx";
import { formatModified, formatSize, typeLabel } from "../format";
import type { Sort, SortColumn } from "../sort";
import { usePreview } from "../usePreview";
import { useRevealOnMount } from "../useRevealOnMount";
import { useSortedEntries } from "../useSortedEntries";
import { EmptyFolder } from "./EmptyFolder";
import { EntryIcon } from "./EntryIcon";
import { entryProps } from "./entryProps";
import { StatusBadge, statusText } from "./StatusBadge";
import styles from "./FileList.module.css";

interface Props {
  entries: Entry[];
  loading: boolean;
  selected: ReadonlySet<string>;
  onSelect: (path: string, toggle: boolean) => void;
  onClear: () => void;
  sort: Sort;
  onSort: (column: SortColumn) => void;
  onOpen: (entry: Entry) => void;
  onContextMenu: (entry: Entry, x: number, y: number) => void;
}

export const FileList = memo(function FileList({
  entries,
  loading,
  selected,
  onSelect,
  onClear,
  sort,
  onSort,
  onOpen,
  onContextMenu,
}: Props) {
  const sorted = useSortedEntries(entries, sort);
  const firstSelected = sorted.find((entry) => selected.has(entry.path))?.path;
  const selectedRow = useRef<HTMLButtonElement>(null);
  useRevealOnMount(selectedRow);

  if (entries.length === 0 && !loading) return <EmptyFolder />;

  return (
    <div
      className={cx(styles.list, loading && styles.loading)}
      onClick={(e) => {
        if (e.target === e.currentTarget) onClear();
      }}
    >
      <div className={styles.header}>
        <SortHeader column="name" label="Name" sort={sort} onSort={onSort} />
        <SortHeader column="status" label="Status" sort={sort} onSort={onSort} />
        <SortHeader column="type" label="Type" sort={sort} onSort={onSort} className={styles.type} />
        <SortHeader column="size" label="Size" sort={sort} onSort={onSort} className={styles.size} />
        <SortHeader column="modified" label="Modified" sort={sort} onSort={onSort} />
      </div>
      {sorted.map((entry) => (
        <Row
          key={entry.path}
          entry={entry}
          selected={selected.has(entry.path)}
          rowRef={entry.path === firstSelected ? selectedRow : undefined}
          onSelect={onSelect}
          onOpen={onOpen}
          onContextMenu={onContextMenu}
        />
      ))}
    </div>
  );
});

interface SortHeaderProps {
  column: SortColumn;
  label: string;
  sort: Sort;
  onSort: (column: SortColumn) => void;
  className?: string;
}

function SortHeader({ column, label, sort, onSort, className }: SortHeaderProps) {
  const active = sort.column === column;
  const Arrow = sort.direction === "asc" ? ChevronUp : ChevronDown;
  const arrow = active && <Arrow size={14} strokeWidth={2.4} aria-hidden />;
  return (
    <button
      type="button"
      className={cx(styles.sortButton, active && styles.sorted, className)}
      aria-pressed={active}
      title={active ? `Sorted by ${label.toLowerCase()}, ${sort.direction === "asc" ? "ascending" : "descending"}` : `Sort by ${label.toLowerCase()}`}
      onClick={() => onSort(column)}
    >
      {column === "size" && arrow}
      {label}
      {column !== "size" && arrow}
    </button>
  );
}

interface RowProps {
  entry: Entry;
  selected: boolean;
  rowRef?: Ref<HTMLButtonElement>;
  onSelect: (path: string, toggle: boolean) => void;
  onOpen: (entry: Entry) => void;
  onContextMenu: (entry: Entry, x: number, y: number) => void;
}

const Row = memo(function Row({ entry, selected, rowRef, onSelect, onOpen, onContextMenu }: RowProps) {
  const { wanted, loaded, onLoad, onError } = usePreview(entry);
  return (
    <button
      ref={rowRef}
      type="button"
      className={cx(styles.row, selected && styles.selected, !entry.readable && styles.unreadable)}
      {...entryProps(entry, { selected, onSelect, onOpen, onContextMenu })}
    >
      <span className={styles.nameCell}>
        <span className={styles.icon}>
          <EntryIcon entry={entry} size={22} />
          {wanted && (
            <img
              className={cx(
                styles.thumb,
                loaded && styles.loaded,
                entry.category === "document" && styles.contain,
              )}
              src={previewUrl(entry.path)}
              alt=""
              draggable={false}
              loading="lazy"
              decoding="async"
              onLoad={onLoad}
              onError={onError}
            />
          )}
        </span>
        <span className={styles.name}>{entry.name}</span>
      </span>
      <span className={styles.statusCell}>
        <StatusBadge entry={entry} />
        <span className={styles.truncate}>{statusText(entry)}</span>
      </span>
      <span className={cx(styles.cell, styles.type)}>{typeLabel(entry)}</span>
      <span className={cx(styles.cell, styles.size)}>{entry.size === null ? "" : formatSize(entry.size)}</span>
      <span className={styles.cell}>{entry.modified === null ? "" : formatModified(entry.modified)}</span>
    </button>
  );
});

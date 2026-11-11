import { memo, useLayoutEffect, useRef, useState, type RefObject } from "react";
import { ChevronDown, ChevronUp } from "lucide-react";
import { type Entry, type PreviewBox } from "../api";
import { cx } from "../cx";
import { formatModified, formatSize, typeLabel } from "../format";
import type { Sort, SortColumn } from "../sort";
import { useArrowKeys } from "../useArrowKeys";
import { usePreview } from "../usePreview";
import { usePreviewOrder } from "../usePreviewOrder";
import { useVirtualRows } from "../virtual";
import { EntryIcon } from "./EntryIcon";
import { entryProps } from "./entryProps";
import { StatusBadge, statusColumnText, statusText } from "./StatusBadge";
import styles from "./FileList.module.css";

interface Props {
  entries: Entry[];
  loading: boolean;
  selected: ReadonlySet<string>;
  active: string | null;
  onSelect: (path: string, toggle: boolean, range?: boolean) => void;
  onMove: (path: string, extend: boolean) => void;
  sort: Sort;
  onSort: (column: SortColumn) => void;
  onOpen: (entry: Entry) => void;
  onContextMenu: (entry: Entry, x: number, y: number) => void;
}

const PICTURE_BOX: PreviewBox = { width: 26, height: 26, fit: "cover" };
const PAGE_BOX: PreviewBox = { ...PICTURE_BOX, fit: "contain" };

export const FileList = memo(function FileList({
  entries,
  loading,
  selected,
  active,
  onSelect,
  onMove,
  sort,
  onSort,
  onOpen,
  onContextMenu,
}: Props) {
  const onKeyDown = useArrowKeys({
    items: entries,
    selectable: (entry) => entry.readable,
    active,
    columns: oneColumn,
    onMove,
  });

  const list = useRef<HTMLDivElement>(null);
  useNarrowAt(list);
  const onSeen = usePreviewOrder(entries, PICTURE_BOX);
  const [shownFirst] = useState(() => (selected.size === 0 ? null : (entries.find((entry) => selected.has(entry.path))?.path ?? null)));
  const { start, end, before, after } = useVirtualRows(list, entries, {
    onSeen,
    selectable: isReadable,
    columns: oneColumn,
    reveal: shownFirst,
  });

  return (
    <div
      ref={list}
      className={cx(styles.list, loading && styles.loading)}
      tabIndex={-1}
      data-scroller
      data-region="files"
      data-keep-motion
      onKeyDown={onKeyDown}
    >
      <div className={styles.header} data-no-marquee>
        <SortHeader column="name" label="Name" sort={sort} onSort={onSort} />
        <SortHeader column="status" label="Status" sort={sort} onSort={onSort} className={styles.status} />
        <SortHeader column="type" label="Type" sort={sort} onSort={onSort} className={styles.type} />
        <SortHeader column="size" label="Size" sort={sort} onSort={onSort} className={styles.size} />
        <SortHeader column="modified" label="Modified" sort={sort} onSort={onSort} className={styles.modified} />
      </div>
      {before > 0 && <div style={{ height: before }} aria-hidden />}
      {entries.slice(start, end).map((entry) => (
        <Row
          key={entry.path}
          entry={entry}
          selected={selected.has(entry.path)}
          leader={entry.path === active}
          onSelect={onSelect}
          onOpen={onOpen}
          onContextMenu={onContextMenu}
        />
      ))}
      {after > 0 && <div style={{ height: after }} aria-hidden />}
    </div>
  );
});

const NARROW_AT = [760, 652, 560, 380];

function useNarrowAt(list: RefObject<HTMLDivElement | null>) {
  useLayoutEffect(() => {
    const element = list.current;
    if (!element) return;
    const style = getComputedStyle(element);
    const padding = parseFloat(style.paddingLeft) + parseFloat(style.paddingRight);
    const apply = () => {
      const width = element.getBoundingClientRect().width - (element.offsetWidth - element.clientWidth) - padding;
      const narrow = NARROW_AT.filter((at) => width <= at).join(" ");
      if ((element.dataset.narrow ?? "") === narrow) return;
      if (narrow) element.dataset.narrow = narrow;
      else delete element.dataset.narrow;
    };
    apply();
    const observer = new ResizeObserver(apply);
    observer.observe(element);
    return () => observer.disconnect();
  }, [list]);
}

const isReadable = (entry: Entry) => entry.readable;
const oneColumn = () => 1;

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
      data-tooltip={active ? `Sorted by ${label.toLowerCase()}, ${sort.direction === "asc" ? "ascending" : "descending"}` : `Sort by ${label.toLowerCase()}`}
      onClick={() => onSort(column)}
    >
      {column === "size" && arrow}
      <span className={styles.sortLabel}>{label}</span>
      {column !== "size" && arrow}
    </button>
  );
}

interface RowProps {
  entry: Entry;
  selected: boolean;
  leader: boolean;
  onSelect: (path: string, toggle: boolean, range?: boolean) => void;
  onOpen: (entry: Entry) => void;
  onContextMenu: (entry: Entry, x: number, y: number) => void;
}

const Row = memo(function Row({ entry, selected, leader, onSelect, onOpen, onContextMenu }: RowProps) {
  const { wanted, src, loaded, onLoad, onError } = usePreview(entry, entry.category === "document" ? PAGE_BOX : PICTURE_BOX);
  return (
    <button
      type="button"
      className={cx(styles.row, selected && styles.selected, selected && leader && styles.leader, !entry.readable && styles.unreadable)}
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
              src={src}
              alt=""
              draggable={false}
              decoding="async"
              onLoad={onLoad}
              onError={onError}
            />
          )}
        </span>
        <span className={styles.name} data-clip>{entry.name}</span>
      </span>
      <span className={styles.statusCell} data-tooltip={statusText(entry) ?? undefined}>
        <StatusBadge entry={entry} />
        <span className={cx(styles.truncate, styles.statusText)}>{statusColumnText(entry)}</span>
      </span>
      <span className={cx(styles.cell, styles.type)}>{typeLabel(entry)}</span>
      <span className={cx(styles.cell, styles.size)}>{entry.size === null ? "" : formatSize(entry.size)}</span>
      <span className={cx(styles.cell, styles.modified)}>{entry.modified === null ? "" : formatModified(entry.modified)}</span>
    </button>
  );
});

import { Fragment, memo, useEffect, useLayoutEffect, useRef, useState } from "react";
import { Plus, X } from "lucide-react";
import { type Entry, type PreviewBox } from "../api";
import { cx } from "../cx";
import { nameParts } from "../format";
import { SELECTING_FROM } from "../selection";
import { useArrowKeys } from "../useArrowKeys";
import { usePreview } from "../usePreview";
import { usePreviewOrder } from "../usePreviewOrder";
import { useVirtualRows } from "../virtual";
import { EntryIcon } from "./EntryIcon";
import { entryProps } from "./entryProps";
import { StatusBadge, statusText } from "./StatusBadge";
import styles from "./FileGrid.module.css";

interface Props {
  entries: Entry[];
  loading: boolean;
  selected: ReadonlySet<string>;
  active: string | null;
  onSelect: (path: string, toggle: boolean, range?: boolean) => void;
  onMove: (path: string, extend: boolean) => void;
  onOpen: (entry: Entry) => void;
  onAdd: (entry: Entry) => void;
  onStop: (entry: Entry) => void;
  onContextMenu: (entry: Entry, x: number, y: number) => void;
}

export const FileGrid = memo(function FileGrid({
  entries,
  loading,
  selected,
  active,
  onSelect,
  onMove,
  onOpen,
  onAdd,
  onStop,
  onContextMenu,
}: Props) {
  const onKeyDown = useArrowKeys({
    items: entries,
    selectable: (entry) => entry.readable,
    active,
    columns,
    onMove,
  });

  const grid = useRef<HTMLDivElement>(null);
  const onSeen = usePreviewOrder(entries, PICTURE_BOX);
  const [shownFirst] = useState(() => (selected.size === 0 ? null : (entries.find((entry) => selected.has(entry.path))?.path ?? null)));
  const { start, end, before, after } = useVirtualRows(grid, entries, {
    onSeen,
    selectable: isReadable,
    columns,
    reveal: shownFirst,
  });

  return (
    <div
      ref={grid}
      className={cx(styles.grid, loading && styles.loading)}
      tabIndex={-1}
      data-scroller
      data-region="files"
      data-selecting={selected.size >= SELECTING_FROM || undefined}
      onKeyDown={onKeyDown}
    >
      {before > 0 && <div className={styles.space} style={{ height: before }} aria-hidden />}
      {entries.slice(start, end).map((entry) => (
        <Tile
          key={entry.path}
          entry={entry}
          selected={selected.has(entry.path)}
          leader={entry.path === active}
          onSelect={onSelect}
          onOpen={onOpen}
          onAdd={onAdd}
          onStop={onStop}
          onContextMenu={onContextMenu}
        />
      ))}
      {after > 0 && <div className={styles.space} style={{ height: after }} aria-hidden />}
    </div>
  );
});

const isReadable = (entry: Entry) => entry.readable;

const PICTURE_BOX: PreviewBox = { width: 76, height: 44, fit: "cover" };
const PAGE_BOX: PreviewBox = { ...PICTURE_BOX, fit: "contain" };
const columns = (grid: HTMLElement) => getComputedStyle(grid).gridTemplateColumns.split(" ").length;

interface TileProps {
  entry: Entry;
  selected: boolean;
  leader: boolean;
  onSelect: (path: string, toggle: boolean, range?: boolean) => void;
  onOpen: (entry: Entry) => void;
  onAdd: (entry: Entry) => void;
  onStop: (entry: Entry) => void;
  onContextMenu: (entry: Entry, x: number, y: number) => void;
}

const justAdded = new Map<string, number>();

function noteAdded(path: string) {
  const now = Date.now();
  justAdded.forEach((at, other) => {
    if (now - at >= ENGAGED_MS) justAdded.delete(other);
  });
  justAdded.set(path, now);
}

const ENGAGED_MS = 800;

const pointer = { x: -1, y: -1 };
if (typeof window !== "undefined") {
  const note = (e: PointerEvent) => {
    pointer.x = e.clientX;
    pointer.y = e.clientY;
  };
  window.addEventListener("pointermove", note, { passive: true });
  window.addEventListener("pointerdown", note, { passive: true, capture: true });
}

function StopCorner({ entry, onStop }: { entry: Entry; onStop: (entry: Entry) => void }) {
  return (
    <span
      role="button"
      className={styles.stop}
      data-tooltip="Stop indexing"
      aria-label={`Stop indexing ${entry.name}`}
      onClick={(e) => {
        e.stopPropagation();
        if (e.detail < 2) onStop(entry);
      }}
    >
      <span className={styles.stopDisc}>
        <X size={12} strokeWidth={1.5} absoluteStrokeWidth style={{ transform: "scale(0.8)" }} aria-hidden />
      </span>
    </span>
  );
}

function StoppableCorner({ entry, onStop }: { entry: Entry; onStop: (entry: Entry) => void }) {
  const [engaged, setEngaged] = useState(() => {
    const at = justAdded.get(entry.path);
    justAdded.delete(entry.path);
    return at !== undefined && Date.now() - at < ENGAGED_MS;
  });
  const corner = useRef<HTMLSpanElement>(null);
  useLayoutEffect(() => {
    if (!engaged) return;
    const box = corner.current?.getBoundingClientRect();
    const over = box && pointer.x >= box.left && pointer.x <= box.right && pointer.y >= box.top && pointer.y <= box.bottom;
    if (!over) setEngaged(false);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);
  const [pointedAt, setPointedAt] = useState(false);
  useEffect(() => {
    if (!engaged) return;
    const timer = setTimeout(() => setEngaged(false), ENGAGED_MS);
    return () => clearTimeout(timer);
  }, [engaged]);
  return (
    <span
      ref={corner}
      className={cx(styles.stoppable, engaged && styles.engaged)}
      onMouseLeave={() => setEngaged(false)}
      onPointerEnter={pointedAt ? undefined : () => setPointedAt(true)}
    >
      <span className={styles.status}>
        <StatusBadge entry={entry} />
      </span>
      {(pointedAt || engaged) && <StopCorner entry={entry} onStop={onStop} />}
    </span>
  );
}

function Corner({
  entry,
  pointedAt,
  onAdd,
  onStop,
}: {
  entry: Entry;
  pointedAt: boolean;
  onAdd: (entry: Entry) => void;
  onStop: (entry: Entry) => void;
}) {
  const stoppable = entry.droppable && (entry.index === "indexing" || entry.index === "paused");
  return (
    <>
      {stoppable ? (
        <StoppableCorner entry={entry} onStop={onStop} />
      ) : (
        <StatusBadge entry={entry} />
      )}
      {pointedAt && <AddCorner entry={entry} onAdd={onAdd} />}
    </>
  );
}

function AddCorner({ entry, onAdd }: { entry: Entry; onAdd: (entry: Entry) => void }) {
  if (!entry.readable || entry.kind === "drive") return null;
  const off = entry.index === "typeNotIndexed";
  if (!off && entry.index !== "notIndexed") return null;
  return (
    <span
      role={off ? undefined : "button"}
      className={cx(styles.add, off && styles.addOff)}
      data-tooltip={off ? (statusText(entry) ?? undefined) : "Add to index"}
      aria-label={off ? undefined : `Add ${entry.name} to the index`}
      onClick={(e) => {
        e.stopPropagation();
        if (off || e.detail >= 2) return;
        noteAdded(entry.path);
        onAdd(entry);
      }}
    >
      <span className={styles.addDisc}>
        <Plus size={12} strokeWidth={1.9} absoluteStrokeWidth style={{ transform: "scale(0.8)" }} aria-hidden />
      </span>
    </span>
  );
}

const Tile = memo(function Tile({
  entry,
  selected,
  leader,
  onSelect,
  onOpen,
  onAdd,
  onStop,
  onContextMenu,
}: TileProps) {
  const usable = entry.readable;

  const { wanted: hasImage, src, loaded: showingImage, onLoad, onError } = usePreview(entry, entry.category === "document" ? PAGE_BOX : PICTURE_BOX);
  const [pointedAt, setPointedAt] = useState(false);
  const corner = <Corner entry={entry} pointedAt={pointedAt} onAdd={onAdd} onStop={onStop} />;

  return (
    <button
      type="button"
      className={cx(styles.tile, selected && styles.selected, selected && leader && styles.leader, !usable && styles.unreadable)}
      data-slide="tile"
      {...entryProps(entry, { selected, onSelect, onOpen, onContextMenu })}
      onPointerEnter={pointedAt ? undefined : () => setPointedAt(true)}
    >
      <span className={styles.preview}>
        <span className={cx(styles.iconBox, showingImage && styles.covered)}>
          <EntryIcon entry={entry} size={40} />
          {!hasImage && <span className={styles.badgeCorner}>{corner}</span>}
        </span>
        {hasImage && (
          <span className={cx(styles.picture, !showingImage && styles.waiting)}>
            <img
              className={cx(
                styles.thumb,
                showingImage && styles.loaded,
                entry.category === "document" && styles.contain,
              )}
              src={src}
              alt=""
              draggable={false}
              decoding="async"
              onLoad={onLoad}
              onError={onError}
            />
            <span className={styles.badgeCorner}>{corner}</span>
          </span>
        )}
      </span>
      <span className={styles.name} data-clip>
        {nameParts(entry.name).map((part, i) => (
          <Fragment key={i}>
            {i > 0 && <wbr />}
            {part}
          </Fragment>
        ))}
      </span>
    </button>
  );
});

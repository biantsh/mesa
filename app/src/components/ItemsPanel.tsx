import { memo, useEffect, useRef, useState, type ReactNode } from "react";
import { X } from "lucide-react";
import type { Listed } from "../api";
import { cx } from "../cx";
import { prefersReducedMotion } from "../motion";
import { parentOf } from "../paths";
import { EntryIcon } from "./EntryIcon";
import { pressProps } from "./entryProps";
import { useArrowKeys } from "../useArrowKeys";
import { IconButton } from "./IconButton";
import { StatusBadge } from "./StatusBadge";
import styles from "./ItemsPanel.module.css";

interface Props {
  title: string;
  items: Listed[];
  selected: ReadonlySet<string>;
  active: string | null;
  onSelect: (path: string, toggle: boolean, range?: boolean) => void;
  onMove: (path: string, extend: boolean) => void;
  onSetSelection: (paths: ReadonlySet<string>) => void;
  onOpen: (item: Listed) => void;
  onContextMenu: (item: Listed, x: number, y: number) => void;
  onRemove: (items: Listed[]) => void;
  verb: string;
  danger?: boolean;
  removeLabel: string;
  empty: string;
  emptyHint: ReactNode;
  collapsed: boolean;
  onExpand?: () => void;
  animate: boolean;
  region: string;
}

const OPENING_MS = 300;
const ROW_IN_MS = 280;
const ROW_OUT_MS = 220;
const seenRows = new WeakSet<HTMLElement>();

function bringIn(row: HTMLElement | null, animate: boolean) {
  if (row === null || seenRows.has(row)) return;
  seenRows.add(row);
  if (!animate || prefersReducedMotion()) return;
  const settled = { opacity: 1, maxHeight: "48px", marginBottom: "4px", transform: "none", overflow: "hidden" };
  row.animate(
    [{ ...settled, opacity: 0, maxHeight: "0px", marginBottom: "0px", transform: "translateY(-6px)" }, settled],
    { duration: ROW_IN_MS, easing: "cubic-bezier(0.22, 0.61, 0.36, 1)" },
  );
}

const leavingRows = new WeakSet<HTMLElement>();

function bringOut(row: HTMLElement | null, done: () => void) {
  if (row === null || leavingRows.has(row)) return;
  leavingRows.add(row);
  const whole = { opacity: 1, maxHeight: "48px", marginBottom: "4px", overflow: "hidden" };
  const gone = { ...whole, opacity: 0, maxHeight: "0px", marginBottom: "0px" };
  const animation = row.animate(
    [whole, { ...whole, opacity: 0, maxHeight: "30px", marginBottom: "2px", offset: 0.45 }, gone],
    { duration: ROW_OUT_MS, easing: "ease-out", fill: "forwards" },
  );
  animation.onfinish = done;
}

interface Ghost {
  item: Listed;
  index: number;
}

function showRows(list: HTMLElement | null, paths: string[]) {
  if (list === null) return;
  const rows = new Map(
    Array.from(list.querySelectorAll<HTMLElement>("[data-path]")).map((row) => [row.dataset.path ?? "", row]),
  );
  const first = rows.get(paths[0])?.closest<HTMLElement>("[data-row]");
  const last = rows.get(paths[paths.length - 1])?.closest<HTMLElement>("[data-row]");
  if (!first || !last) return;

  const origin = list.getBoundingClientRect().top - list.scrollTop;
  const top = first.getBoundingClientRect().top - origin;
  const bottom = last.getBoundingClientRect().bottom - origin;
  const margin = 6;
  const view = { top: list.scrollTop, bottom: list.scrollTop + list.clientHeight };

  let to = list.scrollTop;
  if (bottom - top > list.clientHeight - 2 * margin || top - margin < view.top) to = top - margin;
  else if (bottom + margin > view.bottom) to = bottom + margin - list.clientHeight;
  if (Math.abs(to - list.scrollTop) < 1) return;
  list.scrollTo({ top: Math.max(0, to), behavior: prefersReducedMotion() ? "auto" : "smooth" });
}

function Title({ title, count }: { title: string; count: number }) {
  return (
    <span className={styles.title}>
      {title}
      <span className={styles.count}>{count}</span>
    </span>
  );
}

function RowText({ item, withLeftOut = false }: { item: Listed; withLeftOut?: boolean }) {
  return (
    <>
      <span className={styles.icon}>
        <EntryIcon entry={item} size={22} />
      </span>
      <span className={styles.text}>
        <span className={styles.name}>{item.name}</span>
        <span className={styles.details}>
          <span className={styles.folder}>{parentOf(item.path)}</span>
          {withLeftOut && item.leftOut > 0 && <span className={styles.leftOut}>· {item.leftOut} left out</span>}
        </span>
      </span>
    </>
  );
}

const isBusy = (item: Listed) => item.index === "indexing" || item.index === "paused";

export const ItemsPanel = memo(function ItemsPanel({
  title,
  items,
  selected,
  active,
  onSelect,
  onMove,
  onSetSelection,
  onOpen,
  onContextMenu,
  onRemove,
  verb,
  danger,
  removeLabel,
  empty,
  emptyHint,
  collapsed,
  onExpand,
  animate,
  region,
}: Props) {
  const chosen = items.filter((item) => selected.has(item.path));
  const onArrowKey = useArrowKeys({ items, selectable: () => true, active, columns: () => 1, onMove });
  const card = useRef<HTMLElement>(null);
  const list = useRef<HTMLDivElement>(null);
  const [before, setBefore] = useState(items);
  const [ghosts, setGhosts] = useState<Ghost[]>([]);
  const [wasCollapsed, setWasCollapsed] = useState(collapsed);
  const [justOpened, setJustOpened] = useState(false);
  if (collapsed !== wasCollapsed) {
    setWasCollapsed(collapsed);
    setJustOpened(wasCollapsed && !collapsed);
  }

  if (items !== before) {
    setBefore(items);
    setJustOpened(false);
    const left = new Set(items.map((item) => item.path));
    const going =
      animate && !collapsed && !prefersReducedMotion()
        ? before.flatMap((item, index) => (left.has(item.path) ? [] : [{ item, index }]))
        : [];
    setGhosts([...ghosts.filter((ghost) => !left.has(ghost.item.path)), ...going]);
  }
  const shown: { item: Listed; ghost: boolean }[] = items.map((item) => ({ item, ghost: false }));
  [...ghosts]
    .sort((a, b) => a.index - b.index)
    .forEach((ghost) => shown.splice(Math.min(ghost.index, shown.length), 0, { item: ghost.item, ghost: true }));

  const seen = useRef<Set<string> | null>(null);
  const toShow = useRef<{ paths: string[]; afterOpening: boolean } | null>(null);
  useEffect(() => {
    const before = seen.current;
    seen.current = new Set(items.map((item) => item.path));
    if (before === null || before.size === 0) return;
    const added = items.filter((item) => !before.has(item.path)).map((item) => item.path);
    if (added.length > 0) toShow.current = { paths: added, afterOpening: collapsed || (toShow.current?.afterOpening ?? false) };
  }, [items, collapsed]);
  useEffect(() => {
    const wanted = toShow.current;
    if (collapsed || wanted === null) return;
    const timer = setTimeout(
      () => {
        toShow.current = null;
        showRows(list.current, wanted.paths);
      },
      animate ? OPENING_MS : 0,
    );
    return () => clearTimeout(timer);
  }, [items, collapsed, animate]);

  return (
    <section
      ref={card}
      tabIndex={-1}
      className={cx(styles.panel, collapsed && styles.collapsed, animate && styles.animate)}
      aria-label={title}
      data-own-keys
      onKeyDown={(e) => {
        if (collapsed) return;
        if (e.key === "Delete" && !e.ctrlKey && !e.metaKey && chosen.length > 0) {
          e.preventDefault();
          onRemove(chosen);
          return;
        }
        if (!(e.ctrlKey || e.metaKey) || e.key.toLowerCase() !== "a") return onArrowKey(e);
        e.preventDefault();
        onSetSelection(e.shiftKey ? new Set() : new Set(items.map((item) => item.path)));
      }}
    >
      <header className={styles.header}>
        {onExpand ? (
          <button
            type="button"
            className={styles.toggle}
            aria-expanded={false}
            title={`Show ${title.toLowerCase()}`}
            onClick={() => {
              onExpand();
              card.current?.focus({ preventScroll: true });
            }}
          >
            <Title title={title} count={items.length} />
          </button>
        ) : (
          <div className={styles.toggle}>
            <Title title={title} count={items.length} />
          </div>
        )}
        <button
          type="button"
          className={cx(styles.action, danger && styles.danger)}
          disabled={items.length === 0}
          onClick={() => onRemove(chosen.length > 0 ? chosen : items)}
        >
          {chosen.length > 0 ? `${verb} ${chosen.length}` : `${verb} all`}
        </button>
      </header>

      {shown.length === 0 ? (
        <div
          key={justOpened ? "opened" : "there"}
          className={cx(styles.empty, justOpened && styles.afterRows)}
          inert={collapsed}
        >
          <span>{empty}</span>
          <span className={styles.emptyHint}>{emptyHint}</span>
        </div>
      ) : (
        <div ref={list} className={styles.list} data-scroller data-region={region} inert={collapsed}>
          {shown.map(({ item, ghost }) =>
            ghost ? (
              <div
                key={item.path}
                ref={(row) => bringOut(row, () => setGhosts((all) => all.filter((it) => it.item.path !== item.path)))}
                className={cx(styles.row, item.missing && styles.missing)}
                inert
                aria-hidden
              >
                <div className={styles.main}>
                  <RowText item={item} />
                </div>
              </div>
            ) : (
            <div
              key={item.path}
              ref={(row) => bringIn(row, animate)}
              data-row
              className={cx(
                styles.row,
                selected.has(item.path) && styles.selected,
                selected.has(item.path) && item.path === active && styles.leader,
                item.missing && styles.missing,
              )}
            >
              <button
                type="button"
                className={styles.main}
                {...pressProps(item, true, item.path, {
                  selected: selected.has(item.path),
                  onSelect,
                  onOpen,
                  onContextMenu,
                })}
              >
                <RowText item={item} withLeftOut />
              </button>
              {isBusy(item) && (
                <span className={styles.busy}>
                  <StatusBadge entry={item} />
                </span>
              )}
              <IconButton className={styles.remove} label={removeLabel} tone="danger" onClick={() => onRemove([item])}>
                <X size={14} aria-hidden />
              </IconButton>
            </div>
            ),
          )}
        </div>
      )}
    </section>
  );
});

import { memo, useEffect, useLayoutEffect, useMemo, useRef, useState, type ComponentType, type CSSProperties, type ReactNode, type RefObject, type UIEvent } from "react";
import { Check, FileText, Film, Image as ImageIcon, Music, X, type LucideProps } from "lucide-react";
import { type Category, type SearchHit } from "../api";
import { cx } from "../cx";
import { formatMonth, formatSize } from "../format";
import type { ResultActions } from "../resultMenu";
import { usePreview } from "../usePreview";
import { pressProps } from "./entryProps";
import { moveAmong, resultWindow, rowTop, type Layout, type ResultKey, type ResultWindow } from "../resultWindow";
import { prefersReducedMotion } from "../motion";
import { NAV_KEYS } from "../navigation";
import { useLatest } from "../useLatest";
import { isForSomethingElse } from "../useSelection";
import { EASE_CURVE, EASE_MS } from "./PanelSplitter";
import styles from "./ResultGrid.module.css";
import { fileManager, shortcut } from "../platform";

export const CATEGORY_ICONS: Record<Category, ComponentType<LucideProps>> = {
  image: ImageIcon,
  video: Film,
  audio: Music,
  document: FileText,
};

export interface ResultMessage {
  text: string;
  hint?: ReactNode;
}

export function MessageLink({ onClick, children }: { onClick: () => void; children: ReactNode }) {
  return (
    <button type="button" className={styles.messageLink} onClick={onClick}>
      {children}
    </button>
  );
}

interface Props {
  paneRef: RefObject<HTMLDivElement | null>;
  batches: SearchHit[][];
  searchId: number;
  more: boolean;
  moreClose: boolean;
  stalled: boolean;
  weakFrom: number | null;
  loadingMore: boolean;
  onMore: () => void;
  shown: boolean;
  busy: boolean;
  message: ResultMessage | null;
  progress: boolean;
  selected: ReadonlySet<string>;
  selection: SearchHit[];
  actions: ResultActions;
  onClearSelection: () => void;
  onSelect: (path: string, toggle: boolean, range?: boolean) => void;
  active: string | null;
  onMove: (path: string, extend: boolean) => void;
  onOpen: (hit: SearchHit) => void;
  onContextMenu: (hit: SearchHit, x: number, y: number) => void;
  noted: boolean;
  away: boolean;
  onAway: (away: boolean) => void;
  panel?: boolean;
  panelColumns?: number;
  onPrefetch?: (hit: SearchHit, pressed: boolean) => void;
}

const AHEAD_PX = 800;
const OVERSCAN_ROWS = 3;
const FIRST = 48;
const MESSAGE_MOVES_MS = 360;
const MOST_MEASURES = 3;
const DIVIDER_PX = 40;
const ENTERING_MS = 700;

interface Measured extends Layout {
  top: number;
}

interface Place {
  x: number;
  y: number;
  top: number;
  bottom: number;
}

function placesOf(grid: HTMLElement): Map<HTMLElement, Place> {
  const places = new Map<HTMLElement, Place>();
  for (const tile of Array.from(grid.querySelectorAll<HTMLElement>("[data-result]"))) {
    const box = tile.getBoundingClientRect();
    if (box.width === 0) continue;
    places.set(tile, { x: box.left + box.width / 2, y: box.top + box.height / 2, top: box.top, bottom: box.bottom });
  }
  return places;
}

const isSeen = (place: Place, view: DOMRect) => place.bottom > view.top && place.top < view.bottom;

interface Slide {
  before: Map<HTMLElement, Place>;
  divider: { element: HTMLElement; width: number; top: number } | null;
  more: { element: HTMLElement; left: number; top: number } | null;
  anchor: { at: number; top: number } | null;
  scrollTo: number | null;
}

function slideFrom(grid: HTMLElement, pane: HTMLElement, active: string | null): Slide {
  const before = placesOf(grid);
  const box = pane.getBoundingClientRect();
  const covered = parseFloat(getComputedStyle(pane).scrollPaddingTop) || 0;
  const seen = new DOMRect(box.left, box.top + covered, box.width, Math.max(0, box.height - covered));
  const shown = Array.from(before).filter(([, place]) => isSeen(place, seen));
  const [tile, place] = shown.find(([one]) => one.dataset.path === active) ?? shown[0] ?? [];
  const divider = grid.querySelector<HTMLElement>("[data-divider]");
  const line = divider?.getBoundingClientRect();
  const more = pane.querySelector<HTMLElement>("[data-more]");
  const button = more?.getBoundingClientRect();
  return {
    before,
    divider: divider && line ? { element: divider, width: line.width, top: line.top } : null,
    more: more && button ? { element: more, left: button.left, top: button.top } : null,
    anchor: tile && place ? { at: Number(tile.dataset.result), top: place.top } : null,
    scrollTo: null,
  };
}

function sameWindow(a: ResultWindow, b: ResultWindow): boolean {
  return a.start === b.start && a.end === b.end && a.divider === b.divider && a.before === b.before && a.after === b.after;
}

function sameMeasure(a: Measured | null, b: Measured): boolean {
  return a !== null && a.columns === b.columns && (["stride", "divider", "gap", "top"] as const).every((key) => Math.abs(a[key] - b[key]) < 0.5);
}

const ALWAYS_SHOWN_ABOVE_PX = 48;
const SCROLL_DOWN_PX = 24;
const SCROLL_UP_PX = 10;

export function ResultGrid({ paneRef, batches, searchId, more, moreClose, stalled, weakFrom, loadingMore, onMore, shown, busy, message, progress, selected, selection, actions, onClearSelection, onSelect, active, onMove, onOpen, onContextMenu, noted, away, onAway, panel = false, panelColumns = 0, onPrefetch }: Props) {
  const last = useRef(0);
  const run = useRef(0);
  const scrolled = (e: UIEvent<HTMLDivElement>) => {
    const top = e.currentTarget.scrollTop;
    see();
    const moved = top - last.current;
    last.current = top;
    if (top < ALWAYS_SHOWN_ABOVE_PX) {
      run.current = 0;
      onAway(false);
      return;
    }
    run.current = Math.sign(moved) === Math.sign(run.current) ? run.current + moved : moved;
    if (run.current > SCROLL_DOWN_PX) onAway(true);
    else if (run.current < -SCROLL_UP_PX) onAway(false);
  };
  useEffect(() => {
    paneRef.current?.scrollTo({ top: 0 });
  }, [paneRef, searchId]);

  const hits = useMemo(() => batches.flat(), [batches]);
  const weakAt = useMemo(() => {
    if (weakFrom === null || !batches.slice(0, weakFrom).some((before) => before.length > 0)) return null;
    return batches.slice(0, weakFrom).reduce((count, batch) => count + batch.length, 0);
  }, [batches, weakFrom]);
  const placeInPage = useMemo(() => batches.flatMap((batch, page) => batch.map((_, index) => ({ page, index }))), [batches]);
  const came = useRef<{ search: number; at: number[] }>({ search: searchId, at: [] });
  if (came.current.search !== searchId) came.current = { search: searchId, at: [] };
  while (came.current.at.length < batches.length) came.current.at.push(performance.now());

  const weakerLine = (
    <div key="weaker" className={styles.weakDivider} role="separator" data-divider>
      Weaker matches
    </div>
  );

  const grid = useRef<HTMLDivElement>(null);
  const alone = message !== null && hits.length === 0;
  const messageBox = useRef<HTMLDivElement>(null);
  const messageWas = useRef<{ top: number; alone: boolean } | null>(null);
  useLayoutEffect(() => {
    const box = messageBox.current;
    const top = box?.getBoundingClientRect().top;
    const was = messageWas.current;
    messageWas.current = box && top !== undefined ? { top, alone } : null;
    if (!box || top === undefined || !was?.alone || alone || Math.abs(was.top - top) < 1) return;
    box.animate([{ transform: `translateY(${was.top - top}px)` }, { transform: "none" }], {
      duration: MESSAGE_MOVES_MS,
      easing: "cubic-bezier(0.32, 0.72, 0, 1)",
    });
  });
  const [measured, setMeasured] = useState<Measured | null>(null);
  const measuring = useRef(false);
  const windowAt = (view: { scrollTop: number; height: number }): ResultWindow =>
    measured
      ? resultWindow(hits.length, weakAt, measured, view.scrollTop - measured.top, view.scrollTop - measured.top + view.height, OVERSCAN_ROWS)
      : { start: 0, end: Math.min(hits.length, FIRST), divider: weakAt !== null && weakAt < FIRST, before: 0, after: 0 };
  const view = useRef({ scrollTop: 0, height: 0 });
  const made = windowAt(view.current);
  const madeNow = useRef(made);
  madeNow.current = made;
  const windowAtNow = useRef(windowAt);
  windowAtNow.current = windowAt;
  const [, redraw] = useState(0);
  const seeing = useRef(0);
  const see = () => {
    if (seeing.current) return;
    seeing.current = requestAnimationFrame(() => {
      seeing.current = 0;
      const pane = paneRef.current;
      if (!pane) return;
      view.current = { scrollTop: pane.scrollTop, height: pane.clientHeight };
      if (!sameWindow(madeNow.current, windowAtNow.current(view.current))) redraw((n) => n + 1);
    });
  };
  const [resized, setResized] = useState(0);
  useEffect(() => {
    const pane = paneRef.current;
    const element = grid.current;
    if (!pane || !element) return;
    const heights = new Map<Element, number>();
    const watching = new ResizeObserver((entries) => {
      let changed = false;
      for (const entry of entries) {
        const height = entry.contentRect.height;
        if (heights.get(entry.target) !== height) changed = true;
        heights.set(entry.target, height);
      }
      if (!changed) return;
      see();
      setResized((n) => n + 1);
    });
    watching.observe(pane);
    watching.observe(element);
    return () => {
      watching.disconnect();
      cancelAnimationFrame(seeing.current);
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [paneRef]);
  const remeasured = useRef(0);
  const measuredFor = useRef("");
  useLayoutEffect(() => {
    const element = grid.current;
    const pane = paneRef.current;
    if (!element || !pane) return;
    const what = `${made.start} ${made.end} ${made.divider} ${hits.length} ${weakAt} ${resized} ${layout}`;
    if (measured && what === measuredFor.current) return;
    measuredFor.current = what;
    const tiles = Array.from(element.querySelectorAll<HTMLElement>("[data-result]"));
    if (tiles.length === 0) return;
    const style = getComputedStyle(element);
    const columns = Math.max(1, style.gridTemplateColumns.split(" ").filter(Boolean).length);
    const gap = parseFloat(style.rowGap) || 0;
    const at = (tile: HTMLElement) => Number(tile.dataset.result);
    const topOf = (tile: HTMLElement) => tile.getBoundingClientRect().top;
    const first = tiles[0];
    const below = tiles.find((tile) => at(tile) === at(first) + columns && (weakAt === null || (at(first) < weakAt) === (at(tile) < weakAt)));
    const stride = below ? topOf(below) - topOf(first) : first.getBoundingClientRect().height + gap;
    if (stride <= 0) return;
    let divider = measured?.divider ?? DIVIDER_PX;
    if (weakAt !== null) {
      const lastClose = tiles.find((tile) => at(tile) === weakAt - 1);
      const firstRest = tiles.find((tile) => at(tile) === weakAt);
      if (lastClose && firstRest) divider = topOf(firstRest) - (topOf(lastClose) + stride);
    }
    const start = element.querySelector<HTMLElement>("[data-rows-start]") ?? first;
    const paneTop = pane.getBoundingClientRect().top + pane.clientTop - pane.scrollTop;
    const next: Measured = { columns, stride, divider, gap, top: topOf(start) - paneTop };
    const changed = !sameMeasure(measured, next) && remeasured.current < MOST_MEASURES;
    if (changed) {
      remeasured.current += 1;
      if (remeasured.current === 1) requestAnimationFrame(() => (remeasured.current = 0));
      measuring.current = true;
      setMeasured(next);
    }
    const pending = slide.current;
    if (pending && pending.scrollTo === null && pending.anchor) {
      const box = pane.getBoundingClientRect();
      const y = next.top + rowTop(pending.anchor.at, weakAt, next);
      const ratio = window.devicePixelRatio || 1;
      pending.scrollTo = Math.max(0, Math.round((box.top + pane.clientTop + y - pending.anchor.top) * ratio) / ratio);
      view.current = { scrollTop: pending.scrollTo, height: pane.clientHeight };
      if (!changed && !sameWindow(made, windowAt(view.current))) {
        measuring.current = true;
        redraw((n) => n + 1);
      }
    }
    if (view.current.height === 0) see();
  });

  const layout = panel ? `beside ${panelColumns}` : "alone";
  const laidOutFor = useRef(layout);
  const slide = useRef<Slide | null>(null);
  const sliding = useRef<Animation[]>([]);
  if (laidOutFor.current !== layout) {
    laidOutFor.current = layout;
    slide.current = grid.current && paneRef.current && !prefersReducedMotion() ? slideFrom(grid.current, paneRef.current, active) : null;
  }
  useLayoutEffect(() => {
    const beingMeasured = measuring.current;
    measuring.current = false;
    const now = slide.current;
    const element = grid.current;
    const pane = paneRef.current;
    if (!now || !element || !pane || beingMeasured) return;
    for (const animation of sliding.current) animation.cancel();
    sliding.current = [];
    if (now.scrollTo !== null && Math.abs(pane.scrollTop - now.scrollTo) >= 1) {
      pane.scrollTop = now.scrollTo;
      last.current = pane.scrollTop;
      view.current = { scrollTop: pane.scrollTop, height: pane.clientHeight };
      if (!sameWindow(madeNow.current, windowAtNow.current(view.current))) {
        now.scrollTo = pane.scrollTop;
        redraw((n) => n + 1);
        return;
      }
    }
    slide.current = null;
    const seen = pane.getBoundingClientRect();
    const after = placesOf(element);
    const go = (target: HTMLElement, keyframes: Keyframe[]) => {
      const animation = target.animate(keyframes, { duration: EASE_MS, easing: EASE_CURVE });
      animation.onfinish = () => animation.cancel();
      sliding.current.push(animation);
    };
    for (const [tile, was] of now.before) {
      const is = after.get(tile);
      if (!is || !(isSeen(was, seen) || isSeen(is, seen))) continue;
      const dx = was.x - is.x;
      const dy = was.y - is.y;
      if (Math.abs(dx) < 0.5 && Math.abs(dy) < 0.5) continue;
      go(tile, [{ transform: `translate(${dx}px, ${dy}px)` }, { transform: "translate(0, 0)" }]);
    }
    const more = pane.querySelector<HTMLElement>("[data-more]");
    if (more && now.more && more === now.more.element) {
      const box = more.getBoundingClientRect();
      const [dx, dy] = [now.more.left - box.left, now.more.top - box.top];
      if (Math.abs(dx) >= 0.5 || Math.abs(dy) >= 0.5) go(more, [{ transform: `translate(${dx}px, ${dy}px)` }, { transform: "translate(0, 0)" }]);
    }
    const divider = element.querySelector<HTMLElement>("[data-divider]");
    if (divider && now.divider && divider === now.divider.element) {
      const box = divider.getBoundingClientRect();
      const dy = now.divider.top - box.top;
      if (Math.abs(box.width - now.divider.width) >= 0.5 || Math.abs(dy) >= 0.5) {
        go(divider, [
          { width: `${now.divider.width}px`, transform: `translateY(${dy}px)` },
          { width: `${box.width}px`, transform: "translateY(0)" },
        ]);
      }
    }
  });
  useEffect(() => () => sliding.current.forEach((animation) => animation.cancel()), []);

  const keys = useLatest({ hits, weakAt, measured, active, onMove, shown });
  useEffect(() => {
    const reveal = (at: number) => {
      const pane = paneRef.current;
      const layout = keys.current.measured;
      if (!pane) return;
      const find = () => pane.querySelector<HTMLElement>(`[data-result="${at}"]`);
      if (!find() && layout) {
        const top = layout.top + rowTop(at, keys.current.weakAt, layout);
        const above = parseFloat(getComputedStyle(pane).scrollPaddingTop) || 0;
        if (top - above < pane.scrollTop) pane.scrollTop = top - above;
        else if (top + layout.stride > pane.scrollTop + pane.clientHeight) pane.scrollTop = top + layout.stride - pane.clientHeight;
      }
      let tries = 4;
      const focus = () => {
        const tile = find();
        if (tile) {
          tile.focus({ preventScroll: true });
          tile.scrollIntoView({ block: "nearest" });
        } else if (--tries > 0) requestAnimationFrame(focus);
      };
      focus();
    };
    const onKeyDown = (e: KeyboardEvent) => {
      const { hits, weakAt, measured, active, onMove, shown } = keys.current;
      if (!shown || active === null || e.ctrlKey || e.altKey || e.metaKey || !NAV_KEYS.has(e.key)) return;
      if (isForSomethingElse(e.target) || !paneRef.current || paneRef.current.closest("[inert]")) return;
      if (e.target instanceof Element && e.target.closest("[role='separator']")) return;
      const from = hits.findIndex((hit) => hit.path === active);
      if (from < 0) return;
      e.preventDefault();
      const to = moveAmong(from, e.key as ResultKey, hits.length, weakAt, measured?.columns ?? 1);
      if (to === null) return;
      onMove(hits[to].path, e.shiftKey);
      reveal(to);
    };
    window.addEventListener("keydown", onKeyDown, true);
    return () => window.removeEventListener("keydown", onKeyDown, true);
  }, [paneRef, keys]);
  const asking = more && (stalled || (!moreClose && weakFrom === null));
  const endless = more && !asking;
  const button = asking && !(message && batches.length === 1 && (batches[0]?.length ?? 0) === 0);
  const end = useRef<HTMLDivElement>(null);
  const [nearEnd, setNearEnd] = useState(false);
  useEffect(() => {
    const pane = paneRef.current;
    const target = end.current;
    if (!pane || !target) return;
    const watching = new IntersectionObserver(([entry]) => setNearEnd(entry.isIntersecting), { root: pane, rootMargin: `0px 0px ${AHEAD_PX}px 0px` });
    watching.observe(target);
    return () => watching.disconnect();
  }, [paneRef]);
  useEffect(() => {
    if (nearEnd && endless && shown && !loadingMore) onMore();
  }, [nearEnd, endless, shown, loadingMore, batches.length, onMore]);
  const counting = shown && selection.length > 0;
  return (
    <>
      <div
        ref={paneRef}
        className={cx(
          styles.pane,
          shown && styles.shown,
          busy && styles.busy,
          progress && styles.progress,
          counting && styles.counting,
          noted && styles.noted,
          away && styles.away,
          panel && styles.besidePanel,
        )}
        inert={!shown}
        onScroll={scrolled}
      >
        <div ref={grid} className={cx(styles.grid, alone && styles.alone, panel && styles.withPanel)}>
          {message && (batches[0]?.length ?? 0) === 0 && (
            <div ref={messageBox} className={styles.message} role="status">
              <p className={styles.messageText}>{message.text}</p>
              {message.hint && <p className={styles.hint}>{message.hint}</p>}
            </div>
          )}
          {made.before > 0 && <div className={styles.space} style={{ height: made.before }} data-rows-start aria-hidden />}
          {hits.slice(made.start, made.end).flatMap((hit, offset) => {
            const at = made.start + offset;
            const { page, index } = placeInPage[at];
            return [
              at === weakAt && made.divider && weakerLine,
              <Result
                key={hit.path}
                hit={hit}
                at={at}
                index={index}
                entering={performance.now() - came.current.at[page] < ENTERING_MS}
                selected={selected.has(hit.path)}
                onSelect={onSelect}
                onOpen={onOpen}
                onContextMenu={onContextMenu}
                onPrefetch={onPrefetch}
              />,
            ];
          })}
          {made.divider && made.end === weakAt && weakerLine}
          {made.after > 0 && <div className={styles.space} style={{ height: made.after }} aria-hidden />}
        </div>
        {button && (
          <div className={styles.more}>
            <button type="button" className={styles.moreButton} onClick={onMore} disabled={loadingMore} data-more>
              View more
            </button>
          </div>
        )}
        <div ref={end} aria-hidden />
      </div>
      <div className={cx(styles.cover, shown && styles.shown, noted && styles.noted, away && styles.away)} aria-hidden />
      <SelectionBar
        hits={selection}
        visible={counting}
        progress={progress}
        actions={actions}
        onClear={onClearSelection}
      />
    </>
  );
}

interface BarProps {
  hits: SearchHit[];
  visible: boolean;
  progress: boolean;
  actions: ResultActions;
  onClear: () => void;
}

const COPIED_FOR_MS = 1500;

function Foldable({ shown, inline, children }: { shown: boolean; inline?: boolean; children: ReactNode }) {
  return (
    <div className={cx(styles.fold, inline && styles.inline, !shown && styles.folded)} inert={!shown}>
      <div className={styles.foldInner}>{children}</div>
    </div>
  );
}

function SelectionBar({ hits, visible, progress, actions, onClear }: BarProps) {
  const kept = useRef(hits);
  if (hits.length > 0) kept.current = hits;
  const shown = kept.current;
  const one = shown.length === 1;
  const size = shown.reduce((total, hit) => total + hit.size, 0);

  const [copy, setCopy] = useState<{ what: "files" | "paths"; ok: boolean } | null>(null);
  const timer = useRef<number | undefined>(undefined);
  useEffect(() => () => window.clearTimeout(timer.current), []);
  const copying = (what: "files" | "paths") => {
    const done = (ok: boolean) => {
      setCopy({ what, ok });
      window.clearTimeout(timer.current);
      timer.current = window.setTimeout(() => setCopy(null), COPIED_FOR_MS);
    };
    (what === "files" ? actions.copyFiles(shown) : actions.copyPaths(shown)).then(
      () => done(true),
      () => done(false),
    );
  };
  const copyButton = (what: "files" | "paths", noun: string, shortcut: string) => {
    const result = copy?.what === what ? copy : null;
    return (
      <button type="button" className={styles.barButton} data-tooltip={`Copy ${noun}${one ? "" : "s"} (${shortcut})`} onClick={() => copying(what)}>
        <span className={styles.swap}>
          <span className={cx(result && styles.hidden)}>
            Copy {noun}
            <Foldable inline shown={!one}>
              s
            </Foldable>
          </span>
          <span className={cx(styles.result, !result && styles.hidden)} aria-live="polite">
            {result?.ok === false ? (
              "Couldn’t copy"
            ) : (
              <>
                <Check size={12} strokeWidth={2.5} aria-hidden />
                Copied
              </>
            )}
          </span>
        </span>
      </button>
    );
  };

  return (
    <div className={cx(styles.counter, progress && styles.progress, visible && styles.counting)} aria-hidden={!visible}>
      <div className={styles.bar} data-selection-bar inert={!visible}>
        <span className={styles.count} role="status">
          {shown.length} selected <span className={styles.size}>· {formatSize(size)}</span>
        </span>
        <span className={styles.rule} aria-hidden />
        <Foldable shown={one}>
          <button type="button" className={styles.barButton} data-tooltip="Open" onClick={() => actions.open(shown[0])}>
            Open
          </button>
        </Foldable>
        <Foldable shown={one}>
          <button
            type="button"
            className={styles.barButton}
            data-tooltip={`Show in ${fileManager}`}
            onClick={() => actions.showInFolder(shown[0])}
          >
            Show in {fileManager}
          </button>
        </Foldable>
        {copyButton("files", "file", shortcut("C"))}
        {copyButton("paths", "path", shortcut("C", true))}
        <span className={styles.rule} aria-hidden />
        <button type="button" className={cx(styles.barButton, styles.clear)} onClick={onClear} aria-label="Clear selection" data-tooltip={`Clear selection (${shortcut("A", true)})`}>
          <X size={14} aria-hidden />
        </button>
      </div>
    </div>
  );
}

interface ResultProps {
  hit: SearchHit;
  at: number;
  index: number;
  entering: boolean;
  selected: boolean;
  onSelect: (path: string, toggle: boolean, range?: boolean) => void;
  onOpen: (hit: SearchHit) => void;
  onContextMenu: (hit: SearchHit, x: number, y: number) => void;
  onPrefetch?: (hit: SearchHit, pressed: boolean) => void;
}

const Result = memo(function Result({ hit, at, index, entering, selected, onSelect, onOpen, onContextMenu, onPrefetch }: ResultProps) {
  const Icon = CATEGORY_ICONS[hit.category];
  const { wanted, src, loaded, onLoad, onError } = usePreview({ path: hit.path, modified: hit.modified, kind: "file", readable: true, category: hit.category });
  return (
    <button
      type="button"
      className={cx(styles.tile, selected && styles.selected, !entering && styles.still)}
      style={{ "--i": index } as CSSProperties}
      data-result={at}
      {...pressProps(hit, true, hit.path, { selected, onSelect, onOpen, onContextMenu })}
      onPointerEnter={onPrefetch && (() => onPrefetch(hit, false))}
      onPointerDown={onPrefetch && (() => onPrefetch(hit, true))}
    >
      <span className={styles.preview}>
        <Icon className={styles.icon} size={56} strokeWidth={1} aria-hidden />
        {wanted && (
          <img
            className={cx(styles.thumb, loaded && styles.loaded, hit.category === "document" && styles.contain)}
            src={src}
            alt=""
            draggable={false}
            loading="lazy"
            decoding="async"
            onLoad={onLoad}
            onError={onError}
          />
        )}
      </span>
      <span className={styles.name}>{hit.name}</span>
      <span className={styles.meta}>
        <span className={styles.folder}>{hit.folder}</span>
        <span className={styles.date} aria-hidden>·</span>
        <span className={styles.date}>{formatMonth(hit.modified)}</span>
      </span>
    </button>
  );
});

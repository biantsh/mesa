import { useCallback, useEffect, useLayoutEffect, useRef, useState, type RefObject } from "react";
import { flushSync } from "react-dom";
import { placesOf } from "./entriesShape";

export interface Layout {
  columns: number;
  rowStride: number;
  columnStride: number;
  itemWidth: number;
  itemHeight: number;
  top: number;
  left: number;
  gap: number;
  marginTop: number;
  marginBottom: number;
}

export interface Window {
  start: number;
  end: number;
  before: number;
  after: number;
}

export function windowOf(layout: Layout, count: number, scrollTop: number, height: number, overscan: number): Window {
  const rows = Math.ceil(count / layout.columns);
  if (rows === 0) return { start: 0, end: 0, before: 0, after: 0 };
  const firstSeen = Math.floor((scrollTop - layout.top) / layout.rowStride);
  const lastSeen = Math.floor((scrollTop + height - layout.top) / layout.rowStride);
  const first = Math.min(rows - 1, Math.max(0, firstSeen - overscan));
  const last = Math.max(first, Math.min(rows - 1, lastSeen + overscan));
  const space = (n: number) => (n > 0 ? n * layout.rowStride - layout.gap : 0);
  return {
    start: first * layout.columns,
    end: Math.min(count, (last + 1) * layout.columns),
    before: space(first),
    after: space(rows - 1 - last),
  };
}

export function placeOf(layout: Layout, index: number) {
  const row = Math.floor(index / layout.columns);
  const column = index % layout.columns;
  const left = layout.left + column * layout.columnStride;
  const top = layout.top + row * layout.rowStride;
  return { left, top, right: left + layout.itemWidth, bottom: top + layout.itemHeight };
}

export interface TopItem {
  index: number;
  into: number;
}

export function topItem(layout: Layout, scrollTop: number): TopItem {
  const row = Math.max(0, Math.floor((scrollTop - layout.top) / layout.rowStride));
  return { index: row * layout.columns, into: scrollTop - (layout.top + row * layout.rowStride) };
}

export function scrollTopFor(layout: Layout, top: TopItem): number {
  return Math.max(0, layout.top + Math.floor(top.index / layout.columns) * layout.rowStride + top.into);
}

// WebKit can't scroll a view taller than 33,554,428px, so very tall lists are scrolled through an offset.
export const MOST_SCROLLED = 30_000_000;

export function excessOf(layout: Layout, count: number): number {
  const rows = Math.ceil(count / layout.columns);
  return Math.max(0, (rows > 0 ? rows * layout.rowStride - layout.gap : 0) - MOST_SCROLLED);
}

export interface Anchor {
  scrollTop: number;
  offset: number;
}

export function anchorMargin(layout: Layout, height: number, overscan: number): number {
  return Math.max(height, (overscan + 2) * layout.rowStride);
}

export function anchorAt(layout: Layout, count: number, height: number, excess: number, at: number, overscan: number): Anchor {
  if (excess === 0) return { scrollTop: at, offset: 0 };
  const reach = Math.max(1, MOST_SCROLLED - height);
  const scrollTop = (at * reach) / (reach + excess);
  const margin = anchorMargin(layout, height, overscan);
  if (scrollTop < margin) return { scrollTop: at, offset: 0 };
  if (scrollTop > reach - margin) return { scrollTop: at - excess, offset: excess };
  return { scrollTop, offset: at - scrollTop };
}

export function windowIn(layout: Layout, count: number, anchor: Anchor, height: number, overscan: number, excess: number): Window | null {
  const window = windowOf(layout, count, anchor.scrollTop + anchor.offset, height, overscan);
  if (excess === 0) return window;
  const before = window.before - (window.start > 0 ? anchor.offset : 0);
  const after = window.after - (window.end < count ? excess - anchor.offset : 0);
  const fits =
    (window.start > 0 ? before >= 0 : anchor.offset === 0) && (window.end < count ? after >= 0 : anchor.offset === excess);
  return fits ? { ...window, before, after } : null;
}

export function touchingIn<T extends { path: string }>(
  layout: Layout,
  items: readonly T[],
  can: (item: T) => boolean,
  box: { left: number; top: number; right: number; bottom: number },
): string[] {
  const rows = Math.ceil(items.length / layout.columns);
  const firstRow = Math.max(0, Math.floor((box.top - layout.top - layout.itemHeight) / layout.rowStride) - 1);
  const lastRow = Math.min(rows - 1, Math.floor((box.bottom - layout.top) / layout.rowStride) + 1);
  const sideways = layout.columns > 1 && layout.columnStride > 0;
  const firstColumn = sideways ? Math.max(0, Math.floor((box.left - layout.left - layout.itemWidth) / layout.columnStride) - 1) : 0;
  const lastColumn = sideways ? Math.min(layout.columns - 1, Math.floor((box.right - layout.left) / layout.columnStride) + 1) : layout.columns - 1;
  const paths: string[] = [];
  for (let row = firstRow; row <= lastRow; row++) {
    for (let column = firstColumn; column <= lastColumn; column++) {
      const index = row * layout.columns + column;
      if (index >= items.length || !can(items[index])) continue;
      const place = placeOf(layout, index);
      if (place.right >= box.left && place.left <= box.right && place.bottom >= box.top && place.top <= box.bottom) paths.push(items[index].path);
    }
  }
  return paths;
}

export function nextAnchor(
  layout: Layout,
  count: number,
  height: number,
  was: { offset: number; lastTop: number; scrollTop: number },
  target: { scrollTop: number } | { at: number },
  overscan = OVERSCAN,
): Anchor {
  const excess = excessOf(layout, count);
  if (excess === 0) return { scrollTop: "at" in target ? target.at : target.scrollTop, offset: 0 };
  const reach = Math.max(1, MOST_SCROLLED - height);
  const margin = anchorMargin(layout, height, overscan);
  const kept = Math.min(excess, Math.max(0, was.offset));
  let anchor: Anchor;
  if ("at" in target) {
    const scrollTop = target.at - kept;
    const near = Math.abs(scrollTop - was.scrollTop) <= 2 * height && scrollTop >= 0 && scrollTop <= reach;
    anchor = near ? { scrollTop, offset: kept } : anchorAt(layout, count, height, excess, target.at, overscan);
  } else if (Math.abs(target.scrollTop - was.lastTop) > 2 * height) {
    anchor = anchorAt(layout, count, height, excess, (target.scrollTop * (reach + excess)) / reach, overscan);
  } else {
    anchor = { scrollTop: target.scrollTop, offset: kept };
  }
  const nearEnd = (anchor.offset > 0 && anchor.scrollTop < margin) || (anchor.offset < excess && anchor.scrollTop > reach - margin);
  if (nearEnd || !windowIn(layout, count, anchor, height, overscan, excess)) {
    anchor = anchorAt(layout, count, height, excess, anchor.scrollTop + anchor.offset, overscan);
  }
  return anchor;
}

export interface VirtualView {
  touching(box: { left: number; top: number; right: number; bottom: number }): string[];
  reveal(path: string): void;
  settle(): void;
}

export const virtualViews = new WeakMap<HTMLElement, VirtualView>();

const OVERSCAN = 3;
const FIRST = 120;

function origin(view: HTMLElement) {
  const r = view.getBoundingClientRect();
  return { x: r.left + view.clientLeft - view.scrollLeft, y: r.top + view.clientTop - view.scrollTop };
}

export function useVirtualRows<T extends { path: string }>(
  view: RefObject<HTMLElement | null>,
  items: readonly T[],
  {
    selectable,
    columns,
    reveal,
    onSeen,
  }: { selectable: (item: T) => boolean; columns: (view: HTMLElement) => number; reveal: string | null; onSeen?: (start: number, end: number) => void },
): Window {
  const [shown, setShown] = useState<Window>(() => ({ start: 0, end: Math.min(items.length, FIRST), before: 0, after: 0 }));
  const layout = useRef<Layout | null>(null);
  const latest = useRef({ items, selectable, columns, shown, onSeen });
  latest.current = { items, selectable, columns, shown, onSeen };
  const revealed = useRef(false);

  const measure = useCallback((): Layout | null => {
    const element = view.current;
    if (!element) return null;
    const made = element.querySelectorAll<HTMLElement>("[data-path]");
    const first = made[0];
    if (!first) return null;
    const columnCount = Math.max(1, latest.current.columns(element));
    const at = origin(element);
    const box = first.getBoundingClientRect();
    const beside = columnCount > 1 ? made[1]?.getBoundingClientRect() : undefined;
    const under = made[columnCount]?.getBoundingClientRect();
    const rowStride = under ? under.top - box.top : (layout.current?.rowStride ?? 0);
    if (rowStride <= 0) return layout.current;
    const gap = parseFloat(getComputedStyle(element).rowGap) || 0;
    const { before, start } = latest.current.shown;
    const style = getComputedStyle(first);
    return {
      columns: columnCount,
      rowStride,
      columnStride: beside ? beside.left - box.left : 0,
      itemWidth: box.width,
      itemHeight: box.height,
      top: box.top - at.y - (start > 0 ? before + gap : 0),
      left: box.left - at.x,
      gap,
      marginTop: parseFloat(style.scrollMarginTop) || 0,
      marginBottom: parseFloat(style.scrollMarginBottom) || 0,
    };
  }, [view]);

  const remeasureNow = useRef<(() => void) | null>(null);

  const keptTop = useRef<{ top: TopItem; scrollTop: number } | null>(null);

  const scrollAfter = useRef<number | null>(null);

  const tellSeen = useCallback(
    (now: Layout, at: number) => {
      const { items: all, onSeen: tell } = latest.current;
      const element = view.current;
      if (!tell || !element) return;
      const seen = windowOf(now, all.length, at, element.clientHeight, 0);
      tell(seen.start, seen.end);
    },
    [view],
  );

  const offset = useRef(0);
  const lastTop = useRef(0);

  const anchorFor = useCallback(
    (now: Layout, element: HTMLElement, target: { scrollTop: number } | { at: number }): Anchor =>
      nextAnchor(now, latest.current.items.length, element.clientHeight, { offset: offset.current, lastTop: lastTop.current, scrollTop: element.scrollTop }, target),
    [],
  );

  const update = useCallback(
    (target?: { at: number } | { anchor: Anchor }) => {
      const element = view.current;
      const now = layout.current;
      if (!element || !now) return;
      const anchor =
        target && "anchor" in target ? target.anchor : anchorFor(now, element, target ?? { scrollTop: element.scrollTop });
      if ((target !== undefined && "at" in target) || anchor.scrollTop !== element.scrollTop) {
        const scrollTop = anchor.scrollTop;
        element.scrollTop = scrollTop;
        scrollAfter.current = Math.abs(element.scrollTop - scrollTop) > 1 ? scrollTop : null;
      }
      offset.current = anchor.offset;
      lastTop.current = anchor.scrollTop;
      const count = latest.current.items.length;
      const at = anchor.scrollTop + anchor.offset;
      const next =
        windowIn(now, count, anchor, element.clientHeight, OVERSCAN, excessOf(now, count)) ?? windowOf(now, count, at, element.clientHeight, OVERSCAN);
      tellSeen(now, at);
      setShown((current) =>
        current.start === next.start && current.end === next.end && current.before === next.before && current.after === next.after
          ? current
          : next,
      );
    },
    [view, tellSeen, anchorFor],
  );

  const scrollTo = useCallback(
    (index: number) => {
      const element = view.current;
      const now = layout.current;
      if (!element || !now || index < 0) return;
      const place = placeOf(now, index);
      const above = place.top - now.marginTop;
      const below = place.bottom + now.marginBottom - element.clientHeight;
      const at = element.scrollTop + offset.current;
      if (above < at) update({ at: above });
      else if (below > at) update({ at: below });
      else update();
    },
    [view, update],
  );

  useLayoutEffect(() => {
    const element = view.current;
    if (element && scrollAfter.current !== null) element.scrollTop = scrollAfter.current;
    scrollAfter.current = null;
  }, [view, shown]);

  useLayoutEffect(() => {
    const element = view.current;
    if (!element) return;
    const measured = { width: -1, height: -1 };
    const remeasure = (remeasureNow.current = () => {
      const was = layout.current;
      const next = measure();
      if (!next) return;
      layout.current = next;
      measured.width = element.clientWidth;
      measured.height = element.clientHeight;
      if (!revealed.current) {
        revealed.current = true;
        const at = reveal === null ? -1 : latest.current.items.findIndex((item) => item.path === reveal);
        if (at >= 0) return scrollTo(at);
      }
      if (was && was.columns !== next.columns) {
        const kept = keptTop.current;
        const at = element.scrollTop + offset.current;
        const top = kept && Math.abs(kept.scrollTop - at) <= 1 ? kept.top : topItem(was, at);
        const scrollTop = scrollTopFor(next, top);
        keptTop.current = { top, scrollTop };
        return update({ at: scrollTop });
      }
      update();
    });
    remeasure();
    const observer = new ResizeObserver(() => {
      if (element.clientWidth !== measured.width || element.clientHeight !== measured.height) remeasure();
    });
    observer.observe(element);
    return () => {
      observer.disconnect();
      remeasureNow.current = null;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [view, measure, update, scrollTo]);

  useEffect(() => {
    const element = view.current;
    if (!element) return;
    let frame = 0;
    const onScroll = () => {
      if (!frame) frame = requestAnimationFrame(() => {
        frame = 0;
        const now = layout.current;
        if (!now) return;
        const { items: all, shown: current } = latest.current;
        const anchor = anchorFor(now, element, { scrollTop: element.scrollTop });
        const next = windowIn(now, all.length, anchor, element.clientHeight, OVERSCAN, excessOf(now, all.length));
        const moved = anchor.scrollTop !== element.scrollTop || anchor.offset !== offset.current;
        if (!moved && next && next.start === current.start && next.end === current.end && next.before === current.before && next.after === current.after) {
          lastTop.current = anchor.scrollTop;
          tellSeen(now, anchor.scrollTop + anchor.offset);
          return;
        }
        flushSync(() => update({ anchor }));
      });
    };
    element.addEventListener("scroll", onScroll, { passive: true });
    return () => {
      element.removeEventListener("scroll", onScroll);
      cancelAnimationFrame(frame);
    };
  }, [view, update, tellSeen, anchorFor]);
  useLayoutEffect(() => update(), [items, update]);

  const focused = useRef<HTMLElement | null>(null);
  useEffect(() => {
    const element = view.current;
    if (!element) return;
    const onIn = (e: FocusEvent) => (focused.current = e.target as HTMLElement);
    element.addEventListener("focusin", onIn);
    return () => element.removeEventListener("focusin", onIn);
  }, [view]);
  useLayoutEffect(() => {
    const element = view.current;
    const was = focused.current;
    if (!element || !was || was.isConnected) return;
    focused.current = null;
    const now = document.activeElement;
    if (!now || now === document.body) element.focus({ preventScroll: true });
  }, [view, shown]);

  useEffect(() => {
    const element = view.current;
    if (!element) return;
    virtualViews.set(element, {
      touching: (box) => {
        const now = layout.current;
        if (!now) return [];
        const { items: all, selectable: can } = latest.current;
        return touchingIn(now, all, can, { ...box, top: box.top + offset.current, bottom: box.bottom + offset.current });
      },
      reveal: (path) => scrollTo(placesOf(latest.current.items).get(path) ?? -1),
      settle: () => flushSync(() => remeasureNow.current?.()),
    });
    return () => {
      virtualViews.delete(element);
    };
  }, [view, scrollTo]);

  return shown;
}

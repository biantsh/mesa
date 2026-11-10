import { useCallback, useEffect, useLayoutEffect, useRef, useState, type RefObject } from "react";
import { flushSync } from "react-dom";

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

export interface VirtualView {
  place(): { path: string; left: number; top: number; right: number; bottom: number }[];
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
  { selectable, columns, reveal }: { selectable: (item: T) => boolean; columns: (view: HTMLElement) => number; reveal: string | null },
): Window {
  const [shown, setShown] = useState<Window>(() => ({ start: 0, end: Math.min(items.length, FIRST), before: 0, after: 0 }));
  const layout = useRef<Layout | null>(null);
  const latest = useRef({ items, selectable, columns, shown });
  latest.current = { items, selectable, columns, shown };
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
    const { before } = latest.current.shown;
    const style = getComputedStyle(first);
    return {
      columns: columnCount,
      rowStride,
      columnStride: beside ? beside.left - box.left : 0,
      itemWidth: box.width,
      itemHeight: box.height,
      top: box.top - at.y - (before > 0 ? before + gap : 0),
      left: box.left - at.x,
      gap,
      marginTop: parseFloat(style.scrollMarginTop) || 0,
      marginBottom: parseFloat(style.scrollMarginBottom) || 0,
    };
  }, [view]);

  const remeasureNow = useRef<(() => void) | null>(null);

  const keptTop = useRef<{ top: TopItem; scrollTop: number } | null>(null);

  const scrollAfter = useRef<number | null>(null);

  const update = useCallback((scrollTop?: number) => {
    const element = view.current;
    const now = layout.current;
    if (!element || !now) return;
    if (scrollTop !== undefined) {
      element.scrollTop = scrollTop;
      scrollAfter.current = Math.abs(element.scrollTop - scrollTop) > 1 ? scrollTop : null;
    }
    const next = windowOf(now, latest.current.items.length, scrollTop ?? element.scrollTop, element.clientHeight, OVERSCAN);
    setShown((current) =>
      current.start === next.start && current.end === next.end && current.before === next.before && current.after === next.after
        ? current
        : next,
    );
  }, [view]);

  const scrollTo = useCallback(
    (index: number) => {
      const element = view.current;
      const now = layout.current;
      if (!element || !now || index < 0) return;
      const place = placeOf(now, index);
      const above = place.top - now.marginTop;
      const below = place.bottom + now.marginBottom - element.clientHeight;
      update(above < element.scrollTop ? above : below > element.scrollTop ? below : undefined);
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
        const top = kept && Math.abs(kept.scrollTop - element.scrollTop) <= 1 ? kept.top : topItem(was, element.scrollTop);
        const scrollTop = scrollTopFor(next, top);
        keptTop.current = { top, scrollTop };
        return update(scrollTop);
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
        const next = windowOf(now, all.length, element.scrollTop, element.clientHeight, OVERSCAN);
        if (next.start === current.start && next.end === current.end && next.before === current.before && next.after === current.after) return;
        flushSync(() => update());
      });
    };
    element.addEventListener("scroll", onScroll, { passive: true });
    return () => {
      element.removeEventListener("scroll", onScroll);
      cancelAnimationFrame(frame);
    };
  }, [view, update]);
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
      place: () => {
        const now = layout.current;
        if (!now) return [];
        const { items: all, selectable: can } = latest.current;
        return all.flatMap((item, index) => (can(item) ? [{ path: item.path, ...placeOf(now, index) }] : []));
      },
      reveal: (path) => scrollTo(latest.current.items.findIndex((item) => item.path === path)),
      settle: () => flushSync(() => remeasureNow.current?.()),
    });
    return () => {
      virtualViews.delete(element);
    };
  }, [view, scrollTo]);

  return shown;
}

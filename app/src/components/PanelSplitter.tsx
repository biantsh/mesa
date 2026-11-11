import { useEffect, useLayoutEffect, useRef, useState, type KeyboardEvent, type PointerEvent } from "react";
import { cx } from "../cx";
import { prefersReducedMotion } from "../motion";
import { holdStill } from "../resizing";
import { virtualViews } from "../virtual";
import styles from "./PanelSplitter.module.css";

const MIN = 220;
const MAX = 520;
const FILES_MIN = 320;
const EASE_MS = 260;
const EASE_CURVE = "cubic-bezier(0.32, 0.72, 0, 1)";
const STEP = 16;
const BIG_STEP = 64;

const KEY = "mesa.panelWidth";

function stored(): number | null {
  try {
    const width = Number(localStorage.getItem(KEY));
    return Number.isFinite(width) && width >= MIN ? Math.min(width, MAX) : null;
  } catch {
    return null;
  }
}

function keep(width: number | null) {
  try {
    if (width === null) localStorage.removeItem(KEY);
    else localStorage.setItem(KEY, String(width));
  } catch {
  }
}

const gapOf = (row: HTMLElement) => parseFloat(getComputedStyle(row).columnGap) || 0;

function apply(side: HTMLElement, width: number | null) {
  if (width === null) {
    side.style.removeProperty("--side-width");
    delete side.dataset.sized;
  } else {
    side.style.setProperty("--side-width", `${width}px`);
    side.dataset.sized = "";
  }
}

function widest(side: HTMLElement): number {
  const row = side.parentElement;
  return row ? Math.max(MIN, Math.min(MAX, row.clientWidth - FILES_MIN - gapOf(row))) : MAX;
}

const clamp = (side: HTMLElement, width: number) => Math.round(Math.max(MIN, Math.min(widest(side), width)));

function placeLine(handle: HTMLElement, side: HTMLElement, row: HTMLElement) {
  const scale = window.devicePixelRatio || 1;
  const gap = gapOf(row);
  const left = side.getBoundingClientRect().left;
  const gapPixels = Math.round(gap * scale);
  const parity = gapPixels % 2;
  const nearest = 2 * Math.ceil((2 * scale - parity) / 2 - 0.5) + parity;
  const width = Math.max(1, Math.min(gapPixels, Math.max(nearest, parity === 0 ? 2 : 1)));
  const start = Math.round((left - gap) * scale) + (gapPixels - width) / 2;
  handle.style.setProperty("--line-left", `${start / scale - (left - gap)}px`);
  handle.style.setProperty("--line-width", `${width / scale}px`);
}

const filesBeside = (side: HTMLElement) => (side.previousElementSibling instanceof HTMLElement ? side.previousElementSibling : null);
const showsTiles = (files: HTMLElement) => files.querySelector('[data-slide="tile"]') !== null;

function hold(files: HTMLElement, width: number | null) {
  if (width === null) {
    delete files.dataset.holding;
    files.style.removeProperty("--files-width");
  } else {
    files.style.setProperty("--files-width", `${width}px`);
    files.dataset.holding = "";
  }
}

function pinTiles(files: HTMLElement): (() => void) | null {
  const grid = files.querySelector<HTMLElement>("[data-scroller]");
  if (!grid) return null;
  grid.style.gridTemplateColumns = getComputedStyle(grid).gridTemplateColumns;
  return () => grid.style.removeProperty("grid-template-columns");
}

function settle(files: HTMLElement) {
  const view = files.querySelector<HTMLElement>("[data-scroller]");
  if (view) virtualViews.get(view)?.settle();
}

interface Mover {
  element: HTMLElement;
  x: number;
  y: number;
  tile: boolean;
  top: number;
  bottom: number;
}

function capture(files: HTMLElement): Mover[] {
  const movers: Mover[] = [];
  for (const element of Array.from(files.querySelectorAll<HTMLElement>("[data-slide]"))) {
    const box = element.getBoundingClientRect();
    if (box.width === 0) continue;
    const tile = element.dataset.slide === "tile";
    movers.push({ element, x: tile ? box.left + box.width / 2 : box.left, y: tile ? box.top + box.height / 2 : box.top, tile, top: box.top, bottom: box.bottom });
  }
  return movers;
}

function slides(files: HTMLElement, before: Mover[], after: Mover[]): Animation[] {
  const view = files.querySelector("[data-scroller]")?.getBoundingClientRect();
  const seen = (mover: Mover) => !mover.tile || !view || (mover.bottom > view.top && mover.top < view.bottom);
  const now = new Map(after.map((mover) => [mover.element, mover]));
  const made: Animation[] = [];
  for (const was of before) {
    const to = now.get(was.element);
    if (!to || !(seen(was) || seen(to))) continue;
    const dx = was.x - to.x;
    const dy = was.y - to.y;
    if (Math.abs(dx) < 0.5 && Math.abs(dy) < 0.5) continue;
    const animation = was.element.animate([{ transform: `translate(${dx}px, ${dy}px)` }, { transform: "translate(0, 0)" }], {
      duration: EASE_MS,
      easing: EASE_CURVE,
      fill: "both",
    });
    animation.pause();
    made.push(animation);
  }
  return made;
}

export function PanelSplitter() {
  const [dragging, setDragging] = useState(false);
  const handle = useRef<HTMLDivElement>(null);
  const column = () => handle.current?.parentElement ?? null;
  const current = () => Math.round(column()?.getBoundingClientRect().width ?? 0);
  const drag = useRef<{
    x: number;
    width: number;
    moved: boolean;
    release: () => void;
    frame: number;
    pending: number | null;
    held: { files: HTMLElement; unpin: () => void } | null;
  } | null>(null);
  const easing = useRef<(() => void) | null>(null);
  const sliding = useRef<Animation[]>([]);
  const stopSliding = () => {
    for (const animation of sliding.current) animation.cancel();
    sliding.current = [];
  };

  useLayoutEffect(() => {
    const side = column();
    const row = side?.parentElement;
    if (!side || !row) return;
    const kept = stored();
    if (kept !== null) apply(side, kept);
    const measure = () => {
      handle.current?.setAttribute("aria-valuenow", String(Math.round(side.getBoundingClientRect().width)));
      if (handle.current) placeLine(handle.current, side, row);
    };
    measure();
    const observer = new ResizeObserver(measure);
    observer.observe(side);
    window.addEventListener("resize", measure);
    return () => {
      observer.disconnect();
      window.removeEventListener("resize", measure);
    };
  }, []);

  const endDrag = () => {
    const now = drag.current;
    drag.current = null;
    if (!now) return null;
    cancelAnimationFrame(now.frame);
    const side = column();
    if (side && now.pending !== null) apply(side, now.pending);
    now.release();
    return { moved: now.moved, held: now.held };
  };

  useEffect(
    () => () => {
      easing.current?.();
      const ended = endDrag();
      ended?.held?.unpin();
      stopSliding();
    },
    [],
  );

  const letGo = ({ files, unpin }: { files: HTMLElement; unpin: () => void }) => {
    const animate = !prefersReducedMotion();
    const before = animate ? capture(files) : [];
    unpin();
    settle(files);
    if (!animate) return;
    sliding.current = slides(files, before, capture(files));
    for (const animation of sliding.current) {
      animation.onfinish = () => animation.cancel();
      animation.play();
    }
  };

  const finish = () => {
    setDragging(false);
    const ended = endDrag();
    if (!ended) return;
    if (ended.moved) keep(current());
    if (ended.held) letGo(ended.held);
  };

  const onPointerDown = (e: PointerEvent<HTMLDivElement>) => {
    if (e.button !== 0 || !column()) return;
    easing.current?.();
    stopSliding();
    e.preventDefault();
    e.currentTarget.focus({ preventScroll: true });
    e.currentTarget.setPointerCapture(e.pointerId);
    const side = column()!;
    const files = filesBeside(side);
    const changing: Element[] =
      files && showsTiles(files)
        ? [side, ...Array.from(files.children).filter((child) => !child.matches("[data-scroller]"))]
        : [files && side.parentElement ? side.parentElement : side];
    const releases = changing.map((element) => holdStill(element));
    const release = () => releases.forEach((releaseOne) => releaseOne());
    drag.current = { x: e.clientX, width: current(), moved: false, release, frame: 0, pending: null, held: null };
    setDragging(true);
  };

  const onPointerMove = (e: PointerEvent<HTMLDivElement>) => {
    const now = drag.current;
    const side = column();
    if (!now || !side) return;
    const next = clamp(side, now.width + (now.x - e.clientX));
    now.moved ||= next !== now.width;
    const files = filesBeside(side);
    if (now.moved && !now.held && files && showsTiles(files)) {
      const unpin = pinTiles(files);
      if (unpin) now.held = { files, unpin };
    }
    cancelAnimationFrame(now.frame);
    now.pending = next;
    now.frame = requestAnimationFrame(() => {
      now.pending = null;
      apply(side, next);
    });
  };

  const onKeyDown = (e: KeyboardEvent<HTMLDivElement>) => {
    const side = column();
    if (!side || (e.key !== "ArrowLeft" && e.key !== "ArrowRight")) return;
    e.preventDefault();
    easing.current?.();
    const step = (e.shiftKey ? BIG_STEP : STEP) * (e.key === "ArrowLeft" ? 1 : -1);
    const next = clamp(side, current() + step);
    apply(side, next);
    keep(next);
  };

  const reset = () => {
    const side = column();
    const row = side?.parentElement;
    if (!side || !row) return;
    easing.current?.();
    stopSliding();
    keep(null);
    const files = filesBeside(side);
    const before = files ? capture(files) : [];
    const from = current();
    apply(side, null);
    const to = Math.round(side.getBoundingClientRect().width);
    if (prefersReducedMotion() || to === from) return;
    let moving: Animation[] = [];
    if (files && before.some((mover) => mover.element.dataset.slide === "tile")) {
      const border = files.offsetWidth - files.clientWidth;
      hold(files, row.clientWidth - gapOf(row) - to - border);
      settle(files);
      moving = slides(files, before, capture(files));
    }
    apply(side, from);
    void side.offsetWidth;
    side.dataset.easing = "";
    void side.offsetWidth;

    const end = () => {
      easing.current = null;
      cancelAnimationFrame(frame);
      window.clearTimeout(timer);
      side.removeEventListener("transitionend", onEnd);
      const here = Math.round(side.getBoundingClientRect().width);
      delete side.dataset.easing;
      for (const animation of moving) animation.cancel();
      if (files) hold(files, null);
      if (here === to) {
        apply(side, null);
      } else {
        apply(side, here);
        keep(here);
      }
    };
    const onEnd = (e: TransitionEvent) => {
      if (e.target === side && e.propertyName === "width") end();
    };
    easing.current = end;
    side.addEventListener("transitionend", onEnd);
    const timer = window.setTimeout(end, EASE_MS + 150);
    const frame = requestAnimationFrame(() => {
      apply(side, to);
      for (const animation of moving) animation.play();
    });
  };

  return (
    <div
      ref={handle}
      className={cx(styles.splitter, dragging && styles.dragging)}
      role="separator"
      aria-orientation="vertical"
      aria-label="Width of the lists"
      aria-valuemin={MIN}
      aria-valuemax={MAX}
      tabIndex={0}
      data-tooltip="Drag to resize. Double-click to reset."
      data-no-marquee
      onPointerDown={onPointerDown}
      onPointerMove={onPointerMove}
      onPointerUp={finish}
      onPointerCancel={finish}
      onLostPointerCapture={finish}
      onKeyDown={onKeyDown}
      onDoubleClick={reset}
    />
  );
}

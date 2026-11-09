import { useEffect, useLayoutEffect, useRef, useState, type KeyboardEvent, type PointerEvent } from "react";
import { cx } from "../cx";
import { prefersReducedMotion } from "../motion";
import { holdStill } from "../resizing";
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

interface Mover {
  element: HTMLElement;
  x: number;
  y: number;
}

function capture(files: HTMLElement): Mover[] {
  const movers: Mover[] = [];
  for (const element of Array.from(files.querySelectorAll<HTMLElement>("[data-slide]"))) {
    const box = element.getBoundingClientRect();
    if (box.width === 0) continue;
    const tile = element.dataset.slide === "tile";
    movers.push({ element, x: tile ? box.left + box.width / 2 : box.left, y: tile ? box.top + box.height / 2 : box.top });
  }
  return movers;
}

function slides(before: Mover[], after: Mover[]): Animation[] {
  const now = new Map(after.map((mover) => [mover.element, mover]));
  const made: Animation[] = [];
  for (const was of before) {
    const to = now.get(was.element);
    if (!to) continue;
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
  const drag = useRef<{ x: number; width: number; moved: boolean; release: () => void; frame: number } | null>(null);
  const easing = useRef<(() => void) | null>(null);

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
    if (!now) return false;
    cancelAnimationFrame(now.frame);
    now.release();
    document.body.style.removeProperty("cursor");
    return now.moved;
  };

  useEffect(
    () => () => {
      easing.current?.();
      endDrag();
    },
    [],
  );

  const finish = () => {
    setDragging(false);
    if (endDrag()) keep(current());
  };

  const onPointerDown = (e: PointerEvent<HTMLDivElement>) => {
    if (e.button !== 0 || !column()) return;
    easing.current?.();
    e.preventDefault();
    e.currentTarget.focus({ preventScroll: true });
    e.currentTarget.setPointerCapture(e.pointerId);
    drag.current = { x: e.clientX, width: current(), moved: false, release: holdStill(), frame: 0 };
    document.body.style.setProperty("cursor", "col-resize");
    setDragging(true);
  };

  const onPointerMove = (e: PointerEvent<HTMLDivElement>) => {
    const now = drag.current;
    const side = column();
    if (!now || !side) return;
    const next = clamp(side, now.width + (now.x - e.clientX));
    now.moved ||= next !== now.width;
    cancelAnimationFrame(now.frame);
    now.frame = requestAnimationFrame(() => apply(side, next));
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
    keep(null);
    const files = side.previousElementSibling instanceof HTMLElement ? side.previousElementSibling : null;
    const before = files ? capture(files) : [];
    const from = current();
    apply(side, null);
    const to = Math.round(side.getBoundingClientRect().width);
    if (prefersReducedMotion() || to === from) return;
    let moving: Animation[] = [];
    if (files && before.some((mover) => mover.element.dataset.slide === "tile")) {
      const border = files.offsetWidth - files.clientWidth;
      files.style.setProperty("--files-width", `${row.clientWidth - gapOf(row) - to - border}px`);
      files.dataset.holding = "";
      moving = slides(before, capture(files));
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
      if (files) {
        delete files.dataset.holding;
        files.style.removeProperty("--files-width");
      }
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

import { useEffect, useLayoutEffect, useRef, useState, type KeyboardEvent, type PointerEvent } from "react";
import { flushSync } from "react-dom";
import { cx } from "../cx";
import { holdStill } from "../resizing";
import { nearestPanelColumns, panelRoom, panelWidth } from "../resultWindow";
import styles from "./PanelResizer.module.css";

const PANE_GAP = 6;
const THUMB_INSET = 3;

interface Props {
  track: number;
  taken: number;
  least: number;
  most: number;
  standard: number;
  onResize: (columns: number) => void;
  onAiming: (columns: number | null) => void;
}

export function PanelResizer({ track, taken, least, most, standard, onResize, onAiming }: Props) {
  const handle = useRef<HTMLDivElement>(null);
  const [dragging, setDragging] = useState(false);
  const drag = useRef<{ x: number; from: number; width: number; aim: number; sized: HTMLElement[]; release: () => void; frame: number } | null>(null);
  const panel = () => handle.current?.parentElement ?? null;
  const screen = () => panel()?.parentElement ?? null;
  const widthOf = (columns: number) => panelWidth(track, columns);

  const placeLine = () => {
    const element = handle.current;
    const box = panel();
    if (!element || !box) return;
    const scale = window.devicePixelRatio || 1;
    const rect = box.getBoundingClientRect();
    const onPixel = (at: number) => Math.round(at * scale + 0.005);
    const from = onPixel(rect.left - PANE_GAP) - Math.round(THUMB_INSET * scale);
    const room = onPixel(rect.left) - from;
    const width = Math.max(1, Math.min(room, Math.round((2 * scale - (room % 2)) / 2) * 2 + (room % 2)));
    const start = from + (room - width) / 2;
    element.style.setProperty("--line-left", `${start / scale - element.getBoundingClientRect().left}px`);
    element.style.setProperty("--line-width", `${width / scale}px`);
  };
  useLayoutEffect(() => {
    placeLine();
    const box = panel();
    if (!box) return;
    const watching = new ResizeObserver(placeLine);
    watching.observe(box);
    window.addEventListener("resize", placeLine);
    box.addEventListener("transitionend", placeLine);
    return () => {
      watching.disconnect();
      window.removeEventListener("resize", placeLine);
      box.removeEventListener("transitionend", placeLine);
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // Set only where it's read: on the screen, it would restyle every result each frame.
  const sizedBy = (element: HTMLElement): HTMLElement[] => [panel()!, ...Array.from(element.querySelectorAll<HTMLElement>("[data-panel-room]"))];
  const setWidth = (sized: HTMLElement[], width: number | null) => {
    for (const element of sized) {
      if (width === null) {
        element.style.removeProperty("--panel-drag-width");
        element.style.removeProperty("--panel-drag-room");
      } else {
        element.style.setProperty("--panel-drag-width", `${width}px`);
        element.style.setProperty("--panel-drag-room", `${panelRoom(width)}px`);
      }
    }
  };

  const finish = () => {
    const now = drag.current;
    drag.current = null;
    setDragging(false);
    if (!now) return;
    cancelAnimationFrame(now.frame);
    setWidth(now.sized, now.width);
    now.release();
    void document.body.offsetWidth;
    const nearest = nearestPanelColumns(now.width, track, least, most);
    flushSync(() => {
      onResize(nearest);
      onAiming(null);
    });
    setWidth(now.sized, null);
    placeLine();
  };
  useEffect(
    () => () => {
      const now = drag.current;
      if (!now) return;
      cancelAnimationFrame(now.frame);
      setWidth(now.sized, null);
      now.release();
      onAiming(null);
    },
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [],
  );

  const onPointerDown = (e: PointerEvent<HTMLDivElement>) => {
    const element = screen();
    if (e.button !== 0 || !element) return;
    e.preventDefault();
    e.currentTarget.focus({ preventScroll: true });
    e.currentTarget.dataset.pressed = "";
    e.currentTarget.setPointerCapture(e.pointerId);
    const from = widthOf(taken);
    const sized = sizedBy(element);
    drag.current = { x: e.clientX, from, width: from, aim: taken, sized, release: holdStill(element), frame: 0 };
    setWidth(sized, from);
    setDragging(true);
  };

  const onPointerMove = (e: PointerEvent<HTMLDivElement>) => {
    const now = drag.current;
    if (!now) return;
    now.width = Math.round(Math.max(widthOf(least), Math.min(widthOf(most), now.from - (e.clientX - now.x))));
    cancelAnimationFrame(now.frame);
    now.frame = requestAnimationFrame(() => {
      setWidth(now.sized, now.width);
      placeLine();
    });
    const aim = nearestPanelColumns(now.width, track, least, most);
    if (aim !== now.aim) {
      now.aim = aim;
      onAiming(aim);
    }
  };

  const onKeyDown = (e: KeyboardEvent<HTMLDivElement>) => {
    delete e.currentTarget.dataset.pressed;
    if (e.key !== "ArrowLeft" && e.key !== "ArrowRight") return;
    e.preventDefault();
    const next = Math.max(least, Math.min(most, taken + (e.key === "ArrowLeft" ? 1 : -1)));
    if (next !== taken) onResize(next);
  };

  return (
    <div
      ref={handle}
      className={cx(styles.handle, dragging && styles.dragging)}
      role="separator"
      aria-orientation="vertical"
      aria-label="Width of the panel"
      aria-valuemin={least}
      aria-valuemax={most}
      aria-valuenow={taken}
      aria-valuetext={`${taken} columns`}
      tabIndex={0}
      data-tooltip="Drag to resize. Double-click to reset."
      onPointerDown={onPointerDown}
      onPointerMove={onPointerMove}
      onPointerUp={finish}
      onPointerCancel={finish}
      onLostPointerCapture={finish}
      onKeyDown={onKeyDown}
      onBlur={(e) => delete e.currentTarget.dataset.pressed}
      onPointerEnter={placeLine}
      onFocus={placeLine}
      onDoubleClick={() => {
        const back = Math.max(least, Math.min(most, standard));
        if (taken !== back) onResize(back);
      }}
    />
  );
}

import { useLayoutEffect, useRef, useState, type CSSProperties, type KeyboardEvent, type ReactNode } from "react";
import { cx } from "../cx";
import styles from "./Segmented.module.css";

export interface Segment<T extends string> {
  value: T;
  label: string;
  icon?: ReactNode;
}

interface Props<T extends string> {
  segments: Segment<T>[];
  value: T;
  onChange: (value: T) => void;
  labelledBy: string;
  describedBy?: string;
  slide?: string;
}

interface Place {
  left: number;
  width: number;
}

export function Segmented<T extends string>({ segments, value, onChange, labelledBy, describedBy, slide }: Props<T>) {
  const group = useRef<HTMLDivElement>(null);
  const buttons = useRef<(HTMLButtonElement | null)[]>([]);
  const [place, setPlace] = useState<Place | null>(null);
  const [sliding, setSliding] = useState(false);
  const chosen = segments.findIndex((segment) => segment.value === value);

  useLayoutEffect(() => {
    const box = group.current;
    if (!box) return;
    const measure = () => {
      const button = buttons.current[chosen];
      if (!button) return;
      const first = chosen === 0;
      const last = chosen === segments.length - 1;
      const next = { left: button.offsetLeft - (first ? 1 : 0), width: button.offsetWidth + (first ? 1 : 0) + (last ? 1 : 0) };
      setPlace((now) => (now && now.left === next.left && now.width === next.width ? now : next));
    };
    measure();
    const observer = new ResizeObserver(measure);
    observer.observe(box);
    return () => observer.disconnect();
  }, [chosen, segments.length]);

  useLayoutEffect(() => {
    if (!place || sliding) return;
    const frame = requestAnimationFrame(() => setSliding(true));
    return () => cancelAnimationFrame(frame);
  }, [place, sliding]);

  const move = (e: KeyboardEvent, from: number) => {
    const step = e.key === "ArrowRight" || e.key === "ArrowDown" ? 1 : e.key === "ArrowLeft" || e.key === "ArrowUp" ? -1 : 0;
    if (step === 0) return;
    e.preventDefault();
    const to = (from + step + segments.length) % segments.length;
    onChange(segments[to].value);
    buttons.current[to]?.focus();
  };

  return (
    <div
      ref={group}
      role="radiogroup"
      aria-labelledby={labelledBy}
      aria-describedby={describedBy}
      className={cx(styles.group, sliding && styles.sliding)}
      style={slide ? ({ "--slide": slide } as CSSProperties) : undefined}
    >
      {place && <span className={styles.thumb} style={{ left: place.left, width: place.width }} data-thumb aria-hidden />}
      {segments.map((segment, i) => {
        const isChosen = i === chosen;
        return (
          <button
            key={segment.value}
            ref={(button) => {
              buttons.current[i] = button;
            }}
            type="button"
            role="radio"
            aria-checked={isChosen}
            tabIndex={isChosen ? 0 : -1}
            className={cx(styles.segment, isChosen && styles.chosen)}
            onClick={() => onChange(segment.value)}
            onKeyDown={(e) => move(e, i)}
          >
            {segment.icon}
            {segment.label}
          </button>
        );
      })}
    </div>
  );
}

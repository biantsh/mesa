import { useLayoutEffect, useRef, useState } from "react";
import type { Category } from "../api";
import { cx } from "../cx";
import { TYPE_NAMES, TYPE_ORDER, describeTypes } from "../indexTypes";
import { CATEGORY_ICONS } from "./ResultGrid";
import styles from "./TypeToggles.module.css";

interface Props {
  types: Category[] | null;
  onToggle: (type: Category) => void;
}

const PROGRESS_ROOM_PX = 480;

function tipFor(type: Category, on: boolean): string {
  const words = describeTypes([type]);
  return on ? `Don't index ${words}` : `Index ${words}`;
}

export function TypeToggles({ types, onToggle }: Props) {
  const root = useRef<HTMLDivElement>(null);
  const full = useRef<HTMLDivElement>(null);
  const [compact, setCompact] = useState(true);

  useLayoutEffect(() => {
    const room = root.current?.parentElement;
    const needed = full.current;
    if (!room || !needed) return;
    const fit = () => setCompact(needed.offsetWidth > room.clientWidth - PROGRESS_ROOM_PX);
    fit();
    const observer = new ResizeObserver(fit);
    observer.observe(room);
    observer.observe(needed);
    return () => observer.disconnect();
  }, []);

  const buttons = (labelled: boolean) =>
    TYPE_ORDER.map((type) => {
      const Icon = CATEGORY_ICONS[type];
      const on = types?.includes(type) ?? false;
      return (
        <button
          key={type}
          type="button"
          className={cx(styles.button, on && styles.on)}
          aria-pressed={on}
          aria-label={labelled ? undefined : TYPE_NAMES[type]}
          disabled={types === null}
          data-tooltip={types === null ? undefined : tipFor(type, on)}
          onClick={() => onToggle(type)}
        >
          <Icon size={17} aria-hidden />
          {labelled && <span>{TYPE_NAMES[type]}</span>}
        </button>
      );
    });

  return (
    <div ref={root} className={cx(styles.types, compact && styles.compact)}>
      <div className={styles.group} role="group" aria-label="Kinds of file to index">
        {buttons(!compact)}
      </div>
      <div ref={full} className={cx(styles.group, styles.sizer)} aria-hidden inert>
        {buttons(true)}
      </div>
    </div>
  );
}

import { useLayoutEffect, useRef, type ComponentType } from "react";
import { CircleCheck, CirclePause, Lock, LoaderCircle, type LucideProps } from "lucide-react";
import type { Entry, IndexStatus } from "../api";
import { cx } from "../cx";
import styles from "./StatusBadge.module.css";

interface Badge {
  label: string;
  Icon: ComponentType<LucideProps>;
  size: number;
  strokeWidth?: number;
  className: string;
}

function PartlyIcon({ size = 20 }: LucideProps) {
  return (
    <svg width={size} height={size} viewBox="0 0 24 24" aria-hidden="true">
      <circle cx="12" cy="12" r="11" fill="var(--color-surface)" />
      <circle cx="12" cy="12" r="7.9" fill="none" stroke="currentColor" strokeWidth="2.2" />
      <path d="M12 12V4.1a7.9 7.9 0 1 1-7.9 7.9z" fill="currentColor" />
    </svg>
  );
}

const LOCK = { Icon: Lock, size: 12, strokeWidth: 3 };

const BADGES: Partial<Record<IndexStatus, Badge>> = {
  indexing: { label: "Indexing", Icon: LoaderCircle, size: 18, strokeWidth: 3, className: styles.indexing },
  paused: { label: "Paused", Icon: CirclePause, size: 18, strokeWidth: 2.5, className: styles.paused },
  indexed: { label: "Indexed", Icon: CircleCheck, size: 18, className: styles.indexed },
  partlyIndexed: { label: "Partly indexed", Icon: PartlyIcon, size: 20, className: styles.partly },
  unindexable: { label: "Can't be indexed", ...LOCK, className: styles.lock },
  excluded: { label: "Excluded", ...LOCK, className: styles.lockExcluded },
};

const LOCKED: Badge = { label: "Can't be opened", ...LOCK, className: styles.lock };

function badgeOf(entry: Entry): Badge | null {
  return entry.readable ? (BADGES[entry.index] ?? null) : LOCKED;
}

export function statusText(entry: Entry): string | null {
  return badgeOf(entry)?.label ?? null;
}

export function StatusBadge({ entry }: { entry: Entry }) {
  const badge = badgeOf(entry);
  const badgeElement = useRef<HTMLSpanElement>(null);
  const spinning = badge?.className === styles.indexing;
  useLayoutEffect(() => {
    if (!spinning) return;
    badgeElement.current?.querySelector("svg")?.getAnimations().forEach((animation) => (animation.startTime = 0));
  }, [spinning]);
  if (!badge) return null;
  const { Icon, label } = badge;
  return (
    <span ref={badgeElement} className={cx(styles.badge, badge.className)} role="img" aria-label={label}>
      <Icon size={badge.size} strokeWidth={badge.strokeWidth} />
    </span>
  );
}

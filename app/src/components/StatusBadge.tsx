import type { ComponentType } from "react";
import { CircleCheck, CirclePause, Clock, Lock, LoaderCircle, type LucideProps } from "lucide-react";
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

const BADGES: Partial<Record<IndexStatus, Badge>> = {
  indexing: { label: "Indexing", Icon: LoaderCircle, size: 18, strokeWidth: 2.4, className: styles.indexing },
  queued: { label: "Queued", Icon: Clock, size: 18, strokeWidth: 2.2, className: styles.waiting },
  paused: { label: "Paused", Icon: CirclePause, size: 18, strokeWidth: 2.2, className: styles.paused },
  indexed: { label: "Indexed", Icon: CircleCheck, size: 20, className: styles.indexed },
};

const LOCKED: Badge = {
  label: "Can't be opened",
  Icon: Lock,
  size: 12,
  strokeWidth: 2.4,
  className: styles.lock,
};

function badgeOf(entry: Entry): Badge | null {
  return entry.readable ? (BADGES[entry.index] ?? null) : LOCKED;
}

export function statusText(entry: Entry): string | null {
  return badgeOf(entry)?.label ?? null;
}

export function StatusBadge({ entry }: { entry: Entry }) {
  const badge = badgeOf(entry);
  if (!badge) return null;
  const { Icon, label } = badge;
  return (
    <span className={cx(styles.badge, badge.className)} role="img" aria-label={label}>
      <Icon size={badge.size} strokeWidth={badge.strokeWidth} />
    </span>
  );
}

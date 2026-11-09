import { useLayoutEffect, useRef, type ComponentType, type ReactNode } from "react";
import { CircleCheck, CirclePause, LoaderCircle, type LucideProps } from "lucide-react";
import type { Entry, IndexStatus } from "../api";
import { cx } from "../cx";
import { NO_TYPES_TIP, typeOffTip } from "../indexTypes";
import styles from "./StatusBadge.module.css";

interface Badge {
  label: string;
  columnText?: string;
  Icon: ComponentType<LucideProps>;
  size: number;
  strokeWidth?: number;
  className: string;
}

function Disc({ size, ring, children }: { size: LucideProps["size"]; ring?: boolean; children: ReactNode }) {
  return (
    <svg width={size} height={size} viewBox="0 0 24 24" aria-hidden="true">
      <circle cx="12" cy="12" r="11" fill="var(--color-badge-disc)" />
      {ring && <circle cx="12" cy="12" r="10.5" fill="none" stroke="var(--color-badge-ring)" strokeWidth="1" />}
      {children}
    </svg>
  );
}

function PartlyIcon({ size = 20 }: LucideProps) {
  return (
    <Disc size={size}>
      <circle cx="12" cy="12" r="7.9" fill="none" stroke="currentColor" strokeWidth="2.2" />
      <path d="M12 12V4.1a7.9 7.9 0 1 1-7.9 7.9z" fill="currentColor" />
    </Disc>
  );
}

function LockDiscIcon({ size = 20 }: LucideProps) {
  return (
    <Disc size={size} ring>
      <g
        transform="translate(12 12) scale(0.5) translate(-12 -12)"
        fill="none"
        stroke="currentColor"
        strokeWidth="3.5"
        strokeLinecap="round"
        strokeLinejoin="round"
      >
        <rect width="18" height="11" x="3" y="11" rx="2" ry="2" />
        <path d="M7 11V7a5 5 0 0 1 10 0v4" />
      </g>
    </Disc>
  );
}

const LOCK = { Icon: LockDiscIcon, size: 20 };

const BADGES: Partial<Record<IndexStatus, Badge>> = {
  indexing: { label: "Indexing", Icon: LoaderCircle, size: 18, strokeWidth: 3, className: styles.indexing },
  paused: { label: "Paused", Icon: CirclePause, size: 18, strokeWidth: 2.5, className: styles.paused },
  indexed: { label: "Indexed", Icon: CircleCheck, size: 18, className: styles.indexed },
  partlyIndexed: { label: "Partly indexed", Icon: PartlyIcon, size: 20, className: styles.partly },
  unindexable: { label: "System folder, never indexed", columnText: "Excluded", ...LOCK, className: styles.lockGrey },
  excluded: { label: "Excluded", ...LOCK, className: styles.lockGrey },
};

const LOCKED: Badge = { label: "Can't be opened", ...LOCK, className: styles.lock };

export const PRIVACY_TIP = "Double-click to allow access in System Settings";
const NOT_ALLOWED: Badge = { label: PRIVACY_TIP, columnText: "Not allowed", ...LOCK, className: styles.lock };

function badgeOf(entry: Entry): Badge | null {
  if (!entry.readable) return entry.privacy ? NOT_ALLOWED : LOCKED;
  return BADGES[entry.index] ?? null;
}

export function statusText(entry: Entry): string | null {
  if (entry.readable && entry.index === "typeNotIndexed") return entry.category ? typeOffTip(entry.category) : NO_TYPES_TIP;
  return badgeOf(entry)?.label ?? null;
}

export function statusColumnText(entry: Entry): string | null {
  if (entry.readable && entry.index === "typeNotIndexed") return entry.category ? "Type not indexed" : null;
  const badge = badgeOf(entry);
  return badge ? (badge.columnText ?? badge.label) : null;
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
    <span ref={badgeElement} className={cx(styles.badge, badge.className)} role="img" aria-label={label} data-tooltip={label}>
      <Icon size={badge.size} strokeWidth={badge.strokeWidth} />
    </span>
  );
}

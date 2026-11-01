import { LayoutGrid } from "lucide-react";
import { cx } from "../cx";
import type { ViewMode } from "../useViewMode";
import styles from "./ViewToggle.module.css";

const ROWS = [6, 12, 18];

function ListRows({ size }: { size: number }) {
  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth={2}
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
    >
      {ROWS.map((y) => (
        <g key={y}>
          <rect x="3" y={y - 1.75} width="3.5" height="3.5" rx="1" fill="currentColor" stroke="none" />
          <path d={`M10 ${y}H21`} />
        </g>
      ))}
    </svg>
  );
}

interface Props {
  view: ViewMode;
  onChange: (view: ViewMode) => void;
}

export function ViewToggle({ view, onChange }: Props) {
  return (
    <div className={styles.group} role="group" aria-label="View">
      <button
        type="button"
        className={cx(styles.button, view === "grid" && styles.selected)}
        aria-pressed={view === "grid"}
        aria-label="Grid view"
        title="Grid view"
        onClick={() => onChange("grid")}
      >
        <LayoutGrid size={18} aria-hidden />
      </button>
      <button
        type="button"
        className={cx(styles.button, view === "list" && styles.selected)}
        aria-pressed={view === "list"}
        aria-label="List view"
        title="List view"
        onClick={() => onChange("list")}
      >
        <ListRows size={18} />
      </button>
    </div>
  );
}

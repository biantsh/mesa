import { useEffect, useRef, useState } from "react";
import { ArrowLeft, ChevronRight } from "lucide-react";
import type { Crumb } from "../api";
import type { ViewMode } from "../useViewMode";
import { FillToggle, ViewToggle } from "./ViewToggle";
import styles from "./NavBar.module.css";

interface Props {
  crumbs: Crumb[];
  parent: string | null;
  onOpen: (path: string) => void;
  view: ViewMode;
  onViewChange: (view: ViewMode) => void;
  fill: boolean;
  onFillChange: (fill: boolean) => void;
}

export function NavBar({ crumbs, parent, onOpen, view, onViewChange, fill, onFillChange }: Props) {
  const trail = useRef<HTMLOListElement>(null);
  const listSeen = useRef(false);
  if (view === "list") listSeen.current = true;
  const [shownView, setShownView] = useState(view);
  const [leaving, setLeaving] = useState(false);
  if (view !== shownView) {
    setShownView(view);
    setLeaving(view === "list");
  }

  useEffect(() => {
    trail.current?.scrollTo({ left: trail.current.scrollWidth });
  }, [crumbs]);

  return (
    <div className={styles.bar}>
      <button
        type="button"
        className={styles.up}
        disabled={parent === null}
        onClick={() => parent !== null && onOpen(parent)}
        aria-label="Up one level"
        data-tooltip="Up one level"
      >
        <ArrowLeft size={18} aria-hidden />
      </button>
      <nav className={styles.nav} aria-label="Location">
        <ol ref={trail} className={styles.trail}>
          {crumbs.map((crumb, i) => (
            <li key={crumb.path} className={styles.item}>
              {i > 0 && <ChevronRight size={14} className={styles.separator} aria-hidden />}
              {i === crumbs.length - 1 ? (
                <span className={styles.current} aria-current="page">
                  {crumb.name}
                </span>
              ) : (
                <button type="button" className={styles.crumb} onClick={() => onOpen(crumb.path)}>
                  {crumb.name}
                </button>
              )}
            </li>
          ))}
        </ol>
      </nav>
      <div className={styles.views}>
        {(view === "grid" || leaving) && (
          <FillToggle
            on={fill}
            onChange={onFillChange}
            entering={view === "grid" && listSeen.current}
            leaving={view === "list"}
            onLeft={() => setLeaving(false)}
          />
        )}
        <ViewToggle view={view} onChange={onViewChange} />
      </div>
    </div>
  );
}

import { useEffect, useRef } from "react";
import { ArrowLeft, ChevronRight } from "lucide-react";
import type { Crumb } from "../api";
import type { ViewMode } from "../useViewMode";
import { ViewToggle } from "./ViewToggle";
import styles from "./NavBar.module.css";

interface Props {
  crumbs: Crumb[];
  parent: string | null;
  onOpen: (path: string) => void;
  view: ViewMode;
  onViewChange: (view: ViewMode) => void;
}

export function NavBar({ crumbs, parent, onOpen, view, onViewChange }: Props) {
  const trail = useRef<HTMLOListElement>(null);

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
      <ViewToggle view={view} onChange={onViewChange} />
    </div>
  );
}

import { useEffect, useRef, useState } from "react";
import { ChevronRight } from "lucide-react";
import type { Overview } from "../api";
import { cx } from "../cx";
import { prefersReducedMotion } from "../motion";
import { Bar, figures, Label } from "./IndexProgress";
import { Reveal } from "./Reveal";
import styles from "./IndexStatus.module.css";

interface Props {
  overview: Overview | null;
  onOpenIndex: () => void;
  attention: number;
  raised?: boolean;
  keepsFocus?: boolean;
}

const SWELL_PX = 3;

function PillCount({ paused, count }: { paused: boolean; count: string }) {
  const [arrivedWith] = useState(count !== "");
  const [kept, setKept] = useState(count);
  if (count && count !== kept) setKept(count);
  if (!paused && !count && !kept) return null;
  const separator = paused ? " · " : "";
  return (
    <span className={styles.count}>
      {paused && "Paused"}
      {arrivedWith ? (
        count && `${separator}${count}`
      ) : (
        <Reveal show={count !== ""}>
          <span className={styles.figures}>
            {separator}
            {kept}
          </span>
        </Reveal>
      )}
    </span>
  );
}

export function IndexStatus({ overview, onOpenIndex, attention, raised = false, keepsFocus = raised }: Props) {
  const empty = useRef<HTMLParagraphElement>(null);
  const attended = useRef(attention);
  useEffect(() => {
    if (attention === attended.current) return;
    attended.current = attention;
    const still = prefersReducedMotion();
    const none = { backgroundColor: "transparent", boxShadow: "0 0 0 0 transparent" };
    const lit = {
      backgroundColor: "var(--color-selected-bg)",
      boxShadow: `0 0 0 ${still ? 0 : SWELL_PX}px var(--color-selected-bg)`,
      offset: 0.25,
    };
    empty.current?.animate([none, lit, none], { duration: 500, easing: "ease-out" });
  }, [attention]);

  let content = null;
  if (overview?.progress) {
    const { progress } = overview;
    const count = figures(progress);
    content = (
      <button key="progress" type="button" className={cx(styles.progress, raised && styles.raised)}
        onMouseDown={keepsFocus ? (e) => e.preventDefault() : undefined}
        onClick={onOpenIndex} data-tooltip="Show the index" data-pill>
        <Label progress={progress} />
        <Bar progress={progress} />
        <PillCount paused={progress.paused} count={count} />
        <ChevronRight size={16} className={styles.chevron} aria-hidden />
      </button>
    );
  } else if (overview?.empty) {
    content = (
      <p key="empty" ref={empty} className={styles.empty}>
        Nothing indexed yet.
        <button type="button" className={styles.link} onClick={onOpenIndex}>
          Choose what to index
          <ChevronRight size={14} aria-hidden />
        </button>
      </p>
    );
  }
  return (
    <div className={styles.slot} aria-live="polite">
      {content}
    </div>
  );
}

import { useEffect, useRef } from "react";
import { ChevronRight } from "lucide-react";
import type { Overview } from "../api";
import { Bar, figures, Label } from "./IndexProgress";
import styles from "./IndexStatus.module.css";

interface Props {
  overview: Overview | null;
  onOpenIndex: () => void;
  attention: number;
}

const SWELL_PX = 3;

export function IndexStatus({ overview, onOpenIndex, attention }: Props) {
  const empty = useRef<HTMLParagraphElement>(null);
  const attended = useRef(attention);
  useEffect(() => {
    if (attention === attended.current) return;
    attended.current = attention;
    const still = matchMedia("(prefers-reduced-motion: reduce)").matches;
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
    const text = progress.paused ? (count ? `Paused · ${count}` : "Paused") : count;
    content = (
      <button key="progress" type="button" className={styles.progress} onClick={onOpenIndex} title="Show the index">
        <Label progress={progress} />
        <Bar progress={progress} />
        {text && <span className={styles.count}>{text}</span>}
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

import { useState, type ReactNode } from "react";
import { cx } from "../cx";
import styles from "./Reveal.module.css";

const LOOK: "slide" | "expand" = "expand";

interface Props {
  show: boolean;
  hold?: boolean;
  onExited?: () => void;
  children: ReactNode;
}

export function Reveal({ show, hold = false, onExited, children }: Props) {
  const [present, setPresent] = useState(show);
  if (show && !present) setPresent(true);
  if (!present) return null;

  const leaving = !show;
  return (
    <div
      className={cx(styles.box, styles[LOOK], leaving && styles.leaving, leaving && hold && styles.hold)}
      inert={leaving}
      onAnimationEnd={(e) => {
        if (leaving && e.target === e.currentTarget) {
          setPresent(false);
          onExited?.();
        }
      }}
    >
      <div className={styles.clip}>
        <div className={styles.content}>{children}</div>
      </div>
    </div>
  );
}

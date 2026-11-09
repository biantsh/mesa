import { useEffect, useState } from "react";
import { prefersReducedMotion } from "../motion";
import { cx } from "../cx";
import { Keycaps } from "./Keycaps";
import styles from "./ShortcutTip.module.css";

const EXIT_MS = 200;

interface Props {
  show: boolean;
  keys: string[];
  lifted: boolean;
}

export function ShortcutTip({ show, keys, lifted }: Props) {
  const [present, setPresent] = useState(false);
  if (show && !present) setPresent(true);
  useEffect(() => {
    if (show || !present) return;
    const timer = window.setTimeout(() => setPresent(false), prefersReducedMotion() ? 0 : EXIT_MS);
    return () => window.clearTimeout(timer);
  }, [show, present]);

  if (!present) return null;
  return (
    <div className={cx(styles.tip, lifted && styles.lifted)} role="status" data-leaving={show ? undefined : ""}>
      <span className={styles.text}>
        <strong className={styles.label}>Tip:</strong> Press <Keycaps keys={keys} compact /> from anywhere on your computer to search, even when this app is closed.
      </span>
    </div>
  );
}

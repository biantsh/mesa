import { cx } from "../cx";
import styles from "./Keycaps.module.css";

interface Props {
  keys: string[];
  refusedLast?: boolean;
  compact?: boolean;
}

const SYMBOL = /^[⌃⌥⇧⌘]$/;

export function Keycaps({ keys, refusedLast, compact }: Props) {
  return (
    <span className={cx(styles.keys, compact && styles.compact)} role="img" aria-label={keys.join("+")}>
      {keys.map((key, at) => (
        <kbd key={`${at}${key}`} className={cx(styles.key, SYMBOL.test(key) && styles.symbol, refusedLast && at === keys.length - 1 && styles.refused)}>
          {key}
        </kbd>
      ))}
    </span>
  );
}

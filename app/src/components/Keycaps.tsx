import { cx } from "../cx";
import styles from "./Keycaps.module.css";

interface Props {
  keys: string[];
  name?: string;
  refusedLast?: boolean;
}

const SYMBOL = /^[⌃⌥⇧⌘]$/;

export function Keycaps({ keys, name, refusedLast }: Props) {
  return (
    <span className={styles.keys} role="img" aria-label={name ?? keys.join("+")}>
      {keys.map((key, at) => (
        <kbd key={`${at}${key}`} className={cx(styles.key, SYMBOL.test(key) && styles.symbol, refusedLast && at === keys.length - 1 && styles.refused)}>
          {key}
        </kbd>
      ))}
    </span>
  );
}

import { Check } from "lucide-react";
import type { Category } from "../api";
import { cx } from "../cx";
import { TYPE_NAMES, TYPE_ORDER } from "../indexTypes";
import { CATEGORY_ICONS } from "./ResultGrid";
import styles from "./TypeTiles.module.css";

interface Props {
  checked: Category[];
  locked: Category[];
  disabled?: boolean;
  onToggle: (type: Category) => void;
}

export function TypeTiles({ checked, locked, disabled = false, onToggle }: Props) {
  return (
    <div className={styles.tiles} role="group" aria-label="Kinds of file to index">
      {TYPE_ORDER.map((type) => {
        const Icon = CATEGORY_ICONS[type];
        const on = checked.includes(type);
        const fixed = locked.includes(type);
        return (
          <label
            key={type}
            className={cx(styles.tile, on && styles.on, (fixed || disabled) && styles.fixed)}
            data-tooltip={fixed ? "Already indexed" : undefined}
          >
            <input
              type="checkbox"
              className={styles.input}
              checked={on}
              disabled={fixed || disabled}
              onChange={() => onToggle(type)}
            />
            <Icon size={20} aria-hidden />
            <span className={styles.name}>{TYPE_NAMES[type]}</span>
            <span className={styles.box} aria-hidden>
              {on && <Check size={13} strokeWidth={3.2} />}
            </span>
          </label>
        );
      })}
    </div>
  );
}

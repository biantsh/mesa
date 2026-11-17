import type { Category } from "../api";
import { cx } from "../cx";
import { TYPE_NAMES, TYPE_ORDER, describeTypes } from "../indexTypes";
import { CodeXml } from "lucide-react";
import { CATEGORY_ICONS } from "./ResultGrid";
import styles from "./TypeToggles.module.css";

interface Props {
  types: Category[] | null;
  onToggle: (type: Category) => void;
}

const ICON_SIZE = 17;
const CODE_SIZE = 20;

function tipFor(type: Category, on: boolean): string {
  const words = type === "code" ? "project code" : describeTypes([type]);
  return on ? `Don't index ${words}` : `Index ${words}`;
}

export function TypeToggles({ types, onToggle }: Props) {
  return (
    <div className={styles.types}>
      <div className={styles.group} role="group" aria-label="Kinds of file to index">
        {TYPE_ORDER.map((type) => {
          const Icon = CATEGORY_ICONS[type];
          const on = types?.includes(type) ?? false;
          return (
            <button
              key={type}
              type="button"
              className={cx(styles.button, on && styles.on)}
              aria-pressed={on}
              aria-label={TYPE_NAMES[type]}
              disabled={types === null}
              data-tooltip={types === null ? undefined : tipFor(type, on)}
              onClick={() => onToggle(type)}
            >
              {type === "code" ? (
                <CodeXml size={CODE_SIZE} strokeWidth={(2 * ICON_SIZE) / CODE_SIZE} aria-hidden />
              ) : (
                <Icon size={ICON_SIZE} aria-hidden />
              )}
              <span className={styles.label} aria-hidden>
                {TYPE_NAMES[type]}
              </span>
            </button>
          );
        })}
      </div>
    </div>
  );
}

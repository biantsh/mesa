import { useState, type ComponentType } from "react";
import type { LucideProps } from "lucide-react";
import styles from "./SlidingIcon.module.css";

interface Props<K extends string> {
  icons: Record<K, ComponentType<LucideProps>>;
  current: K;
  size?: number;
}

export function SlidingIcon<K extends string>({ icons, current, size = 18 }: Props<K>) {
  const [seen, setSeen] = useState(current);
  const [previous, setPrevious] = useState<K | null>(null);
  if (current !== seen) {
    setPrevious(seen);
    setSeen(current);
  }
  return (
    <span className={styles.box} style={{ width: size, height: size }} aria-hidden>
      {(Object.keys(icons) as K[]).map((key) => {
        const Icon: ComponentType<LucideProps> = icons[key];
        const lane = key === current ? "current" : key === previous ? "gone" : "waiting";
        return (
          <span key={key} className={styles.icon} data-lane={lane}>
            <Icon size={size} />
          </span>
        );
      })}
    </span>
  );
}

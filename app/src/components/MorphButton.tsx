import { useLayoutEffect, useRef, useState, type ReactNode } from "react";
import { cx } from "../cx";
import { Button, type ButtonProps } from "./Button";
import styles from "./MorphButton.module.css";

const DEFAULT_ICON = 16;
const GAP = 8;

interface Props extends Omit<ButtonProps, "children"> {
  selecting: boolean;
  icon: ReactNode;
  iconSize?: number;
  label: string;
  selectingLabel: string;
}

export function MorphButton({ selecting, icon, iconSize = DEFAULT_ICON, label, selectingLabel, className, ...rest }: Props) {
  const first = useRef<HTMLSpanElement>(null);
  const second = useRef<HTMLSpanElement>(null);
  const [widths, setWidths] = useState({ first: 0, second: 0 });

  useLayoutEffect(() => {
    const measure = () => {
      const next = {
        first: first.current?.getBoundingClientRect().width ?? 0,
        second: second.current?.getBoundingClientRect().width ?? 0,
      };
      setWidths((current) => (current.first === next.first && current.second === next.second ? current : next));
    };
    measure();
    const observer = new ResizeObserver(measure);
    if (first.current) observer.observe(first.current);
    if (second.current) observer.observe(second.current);
    return () => observer.disconnect();
  }, []);

  const iconAt = -(iconSize + GAP + widths.first) / 2;
  const labelAt = selecting ? -widths.second / 2 : iconAt + iconSize + GAP;

  return (
    <Button className={cx(styles.morph, className)} {...rest}>
      <span className={styles.sizers} aria-hidden>
        <span>
          <span className={styles.iconSpace} style={{ width: iconSize }} />
          <span ref={first}>{label}</span>
        </span>
        <span>
          <span ref={second}>{selectingLabel}</span>
        </span>
      </span>
      <span
        className={cx(styles.icon, selecting && styles.iconGone)}
        style={{ left: `calc(50% + ${iconAt}px)` }}
        aria-hidden
      >
        {icon}
      </span>
      <span className={styles.window}>
        <span className={styles.label} style={{ left: `calc(50% + ${labelAt}px)` }}>
          {selecting ? selectingLabel : label}
        </span>
      </span>
    </Button>
  );
}

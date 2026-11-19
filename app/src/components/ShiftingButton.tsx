import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { cx } from "../cx";
import { useSwap } from "../useSwap";
import { Button, type ButtonProps } from "./Button";
import styles from "./ShiftingButton.module.css";

interface Props extends Omit<ButtonProps, "children"> {
  label: string;
}

export function ShiftingButton({ label, className, ...rest }: Props) {
  const { current, previous, changing, gone } = useSwap(label);
  const sizer = useRef<HTMLSpanElement>(null);
  const [width, setWidth] = useState<number | null>(null);
  useLayoutEffect(() => {
    const element = sizer.current;
    if (!element) return;
    const observer = new ResizeObserver(([entry]) => setWidth(entry.borderBoxSize?.[0]?.inlineSize ?? entry.contentRect.width));
    observer.observe(element);
    return () => observer.disconnect();
  }, []);
  const [ready, setReady] = useState(false);
  useEffect(() => {
    if (width === null || ready) return;
    let second = 0;
    const first = requestAnimationFrame(() => {
      second = requestAnimationFrame(() => setReady(true));
    });
    return () => {
      cancelAnimationFrame(first);
      cancelAnimationFrame(second);
    };
  }, [width, ready]);

  return (
    <Button className={cx(styles.shifting, className)} {...rest}>
      <span ref={sizer} className={styles.sizer} aria-hidden>
        {current}
      </span>
      <span className={cx(styles.text, ready && styles.measured)} style={width === null ? undefined : { width }}>
        {previous !== null && (
          <span key={`was:${previous}`} className={cx(styles.layer, styles.leaving)} onAnimationEnd={gone} aria-hidden>
            {previous}
          </span>
        )}
        <span key={current} className={cx(styles.layer, changing && styles.coming)}>
          {current}
        </span>
      </span>
    </Button>
  );
}

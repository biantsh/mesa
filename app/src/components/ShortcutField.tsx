import { useLayoutEffect, useRef, useState, type CSSProperties, type ReactNode, type Ref } from "react";
import { cx } from "../cx";
import { Keycaps } from "./Keycaps";
import styles from "./ShortcutField.module.css";

const PAD = 6;
const ROW_HEIGHT = 28;

type View = "idle" | "prompt" | "live";
type Sizes = Record<View, { width: number; height: number }>;

interface Props {
  listening: boolean;
  children: ReactNode;
  live: string[];
  refusedLast: boolean;
  prompt: string;
  label: string;
  tooltip: string;
  describedBy?: string;
  disabled?: boolean;
  onClick: (event: React.MouseEvent<HTMLButtonElement>) => void;
  ref?: Ref<HTMLButtonElement>;
}

export function ShortcutField({ listening, children, live, refusedLast, prompt, label, tooltip, describedBy, disabled, onClick, ref }: Props) {
  const view: View = !listening ? "idle" : live.length > 0 ? "live" : "prompt";
  const idle = useRef<HTMLSpanElement>(null);
  const asking = useRef<HTMLSpanElement>(null);
  const holding = useRef<HTMLSpanElement>(null);
  const [sizes, setSizes] = useState<Sizes | null>(null);
  const [row, setRow] = useState<{ height: number; paddingRight: number } | null>(null);
  const [ready, setReady] = useState(false);
  const lastLive = useRef<string[]>([]);
  if (live.length > 0) lastLive.current = live;

  useLayoutEffect(() => {
    const layers = { idle: idle.current, prompt: asking.current, live: holding.current };
    const measure = () => {
      const size = (layer: HTMLElement | null) => ({ width: layer?.offsetWidth ?? 0, height: layer?.offsetHeight ?? 0 });
      const next = { idle: size(layers.idle), prompt: size(layers.prompt), live: size(layers.live) };
      setSizes((now) =>
        now && (Object.keys(next) as View[]).every((v) => now[v].width === next[v].width && now[v].height === next[v].height) ? now : next,
      );
    };
    measure();
    const observer = new ResizeObserver(measure);
    for (const layer of Object.values(layers)) if (layer) observer.observe(layer);
    return () => observer.disconnect();
  }, []);

  useLayoutEffect(() => {
    const element = idle.current?.closest<HTMLElement>("[data-setting-row]");
    if (!element) return;
    const measure = () => {
      const paddingRight = parseFloat(getComputedStyle(element).paddingRight) || 0;
      setRow((now) => (now && now.height === element.offsetHeight && now.paddingRight === paddingRight ? now : { height: element.offsetHeight, paddingRight }));
    };
    measure();
    const observer = new ResizeObserver(measure);
    observer.observe(element);
    return () => observer.disconnect();
  }, []);

  useLayoutEffect(() => {
    if (!sizes || ready) return;
    const frame = requestAnimationFrame(() => setReady(true));
    return () => cancelAnimationFrame(frame);
  }, [sizes, ready]);

  const pad = listening ? PAD : 0;
  const size = sizes?.[view];
  let style: CSSProperties | undefined;
  if (size) {
    const width = size.width + 2 * pad;
    const height = size.height + 2 * pad;
    const vertical = (ROW_HEIGHT - height) / 2;
    const right = listening && row ? (row.height - height) / 2 - row.paddingRight : 0;
    style = { width, height, margin: `${vertical}px ${right}px ${vertical}px 0` };
  }

  return (
    <button
      ref={ref}
      type="button"
      className={cx(styles.field, ready && styles.ready, listening && styles.listening)}
      style={style}
      disabled={disabled}
      aria-label={label}
      aria-describedby={describedBy}
      aria-pressed={listening}
      data-tooltip={tooltip}
      onClick={onClick}
    >
      <span className={styles.clip}>
        <span ref={idle} className={cx(styles.layer, view === "idle" && styles.seen)}>
          {children}
        </span>
        <span ref={asking} className={cx(styles.layer, view === "prompt" && styles.seen)}>
          <span className={styles.prompt}>{prompt}</span>
        </span>
        <span ref={holding} className={cx(styles.layer, view === "live" && styles.seen)}>
          {lastLive.current.length > 0 && <Keycaps keys={lastLive.current} refusedLast={refusedLast} />}
        </span>
      </span>
    </button>
  );
}

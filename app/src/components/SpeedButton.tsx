import { useEffect, useLayoutEffect, useRef, useState, type CSSProperties } from "react";
import { createPortal } from "react-dom";
import { Zap } from "lucide-react";
import type { Progress } from "../api";
import { prefersReducedMotion } from "../motion";
import { worthATip } from "../speedTip";
import { useLatest } from "../useLatest";
import { useSpeedTip } from "../useSpeedTip";
import { IconButton } from "./IconButton";
import styles from "./SpeedButton.module.css";

const MARGIN_PX = 24;
const GAP_PX = 10;
const EXIT_MS = 200;

interface Props {
  progress: Progress;
  secondsLeft: number | null;
  onScreen: boolean;
  onToggle: () => void;
}

export function SpeedButton({ progress, secondsLeft, onScreen, onToggle }: Props) {
  const [anchor, setAnchor] = useState<HTMLElement | null>(null);
  const [hovered, setHovered] = useState(false);
  const showTip = useSpeedTip({
    onScreen,
    worth: worthATip({ secondsLeft, paused: progress.paused, counting: progress.counting, turbo: progress.turbo }),
    turbo: progress.turbo,
  });
  const [present, setPresent] = useState(false);
  if (showTip && !present) setPresent(true);

  return (
    <span
      ref={setAnchor}
      className={styles.anchor}
      data-pointed={showTip && !hovered ? "" : undefined}
      onPointerEnter={() => setHovered(true)}
      onPointerLeave={() => setHovered(false)}
    >
      <IconButton
        label={progress.turbo ? "Turn off fast indexing" : "Turn on fast indexing (uses more resources)"}
        onClick={onToggle}
        className={styles.speed}
        data-on={progress.turbo ? "" : undefined}
      >
        <Zap size={18} className={styles.bolt} aria-hidden />
      </IconButton>
      {present && anchor && (
        <Tip anchor={anchor} hidden={hovered} leaving={!showTip} onGone={() => setPresent(false)} />
      )}
    </span>
  );
}

function Tip({
  anchor,
  hidden,
  leaving,
  onGone,
}: {
  anchor: HTMLElement;
  hidden: boolean;
  leaving: boolean;
  onGone: () => void;
}) {
  const gone = useLatest(onGone);
  useEffect(() => {
    if (!leaving) return;
    const timer = window.setTimeout(() => gone.current(), prefersReducedMotion() ? 0 : EXIT_MS);
    return () => window.clearTimeout(timer);
  }, [leaving, gone]);

  const tip = useRef<HTMLDivElement>(null);
  const [place, setPlace] = useState<{ left: number; top: number; arrow: number } | null>(null);

  const put = () => {
    const el = tip.current;
    if (!el) return;
    const box = anchor.getBoundingClientRect();
    const width = el.offsetWidth;
    const middle = box.left + box.width / 2;
    const left = Math.min(Math.max(MARGIN_PX, middle - width / 2), window.innerWidth - MARGIN_PX - width);
    const next = { left: Math.round(left), top: Math.round(box.bottom + GAP_PX), arrow: Math.round(middle - left) };
    setPlace((now) => (now && now.left === next.left && now.top === next.top && now.arrow === next.arrow ? now : next));
  };

  useLayoutEffect(put);
  useEffect(() => {
    const watch = window.setInterval(put, 200);
    window.addEventListener("resize", put);
    return () => {
      window.clearInterval(watch);
      window.removeEventListener("resize", put);
    };
  });

  return createPortal(
    <div
      ref={tip}
      className={styles.tip}
      role="status"
      data-hidden={hidden ? "" : undefined}
      data-leaving={leaving ? "" : undefined}
      style={
        place
          ? ({ left: place.left, top: place.top, "--arrow": `${place.arrow}px` } as CSSProperties)
          : { visibility: "hidden" }
      }
    >
      <strong className={styles.label}>Tip:</strong> Press to index faster. It uses more resources, and you can switch
      back any time.
    </div>,
    document.body,
  );
}

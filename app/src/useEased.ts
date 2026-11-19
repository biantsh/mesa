import { useEffect, useRef, useState } from "react";

const CATCH_UP_MS = 180;

export function useEased(target: number, from = 0): number {
  const [value, setValue] = useState(from);
  const shown = useRef(from);
  useEffect(() => {
    if (window.matchMedia("(prefers-reduced-motion: reduce)").matches) {
      shown.current = Math.max(shown.current, target);
      setValue(shown.current);
      return;
    }
    let frame = 0;
    let last = performance.now();
    const step = (now: number) => {
      const gap = target - shown.current;
      if (gap <= 0.0005) {
        shown.current = Math.max(shown.current, target);
        setValue(shown.current);
        return;
      }
      shown.current += gap * (1 - Math.exp(-(now - last) / CATCH_UP_MS));
      last = now;
      setValue(shown.current);
      frame = requestAnimationFrame(step);
    };
    frame = requestAnimationFrame(step);
    return () => cancelAnimationFrame(frame);
  }, [target]);
  return value;
}

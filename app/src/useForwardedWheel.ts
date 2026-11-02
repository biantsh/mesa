import { useCallback, useEffect, useRef, type RefObject, type WheelEvent } from "react";

const DURATION_MS = 170;
const LINE_PX = 40;

export function useForwardedWheel(scroller: RefObject<HTMLElement | null>) {
  const running = useRef<{ frame: number; to: number } | null>(null);

  const stop = useCallback(() => {
    if (running.current) cancelAnimationFrame(running.current.frame);
    running.current = null;
  }, []);

  useEffect(() => {
    const element = scroller.current;
    if (!element) return;
    element.addEventListener("wheel", stop, { passive: true });
    element.addEventListener("pointerdown", stop, { passive: true });
    return () => {
      element.removeEventListener("wheel", stop);
      element.removeEventListener("pointerdown", stop);
      stop();
    };
  }, [scroller, stop]);

  return useCallback(
    (e: WheelEvent) => {
      const element = scroller.current;
      if (!element) return;
      const delta = e.deltaMode === 1 ? e.deltaY * LINE_PX : e.deltaMode === 2 ? e.deltaY * element.clientHeight : e.deltaY;
      const farthest = element.scrollHeight - element.clientHeight;
      const to = Math.min(farthest, Math.max(0, (running.current?.to ?? element.scrollTop) + delta));
      const from = element.scrollTop;
      stop();
      if (to === from) return;
      if (matchMedia("(prefers-reduced-motion: reduce)").matches) {
        element.scrollTop = to;
        return;
      }
      const started = performance.now();
      const step = (now: number) => {
        const done = Math.min(1, (now - started) / DURATION_MS);
        element.scrollTop = from + (to - from) * (1 - (1 - done) ** 3);
        if (done < 1) running.current = { frame: requestAnimationFrame(step), to };
        else running.current = null;
      };
      running.current = { frame: requestAnimationFrame(step), to };
    },
    [scroller, stop],
  );
}

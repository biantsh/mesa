import { useCallback, type RefObject, type WheelEvent } from "react";
import { scrollSmoothly } from "./scrollKeys";

const LINE_PX = 40;

export function useForwardedWheel(scroller: RefObject<HTMLElement | null>) {
  return useCallback(
    (e: WheelEvent) => {
      const element = scroller.current;
      if (!element) return;
      const delta = e.deltaMode === 1 ? e.deltaY * LINE_PX : e.deltaMode === 2 ? e.deltaY * element.clientHeight : e.deltaY;
      scrollSmoothly(element, delta);
    },
    [scroller],
  );
}

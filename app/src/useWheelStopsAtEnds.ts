import { useEffect, type RefObject } from "react";
import { isMac } from "./platform";

// WebView2 at fractional scales scrolls a pixel past the end and then snaps back, so the wheel is ignored at the ends
// (except on macOS, where the bounce is wanted).
export function useWheelStopsAtEnds(view: RefObject<HTMLElement | null>, active: boolean) {
  useEffect(() => {
    const element = view.current;
    if (!active || !element || isMac) return;
    const onWheel = (e: WheelEvent) => {
      if (e.ctrlKey || Math.abs(e.deltaY) <= Math.abs(e.deltaX)) return;
      const atEnd = e.deltaY > 0 ? element.scrollTop >= element.scrollHeight - element.clientHeight - 1 : element.scrollTop <= 0;
      if (atEnd) e.preventDefault();
    };
    element.addEventListener("wheel", onWheel, { passive: false });
    return () => element.removeEventListener("wheel", onWheel);
  }, [view, active]);
}

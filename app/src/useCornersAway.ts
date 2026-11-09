import { useLayoutEffect, useState, type RefObject } from "react";

const CORNER_SIDE_PX = 16;
const PILL_CLEARANCE_PX = 16;

export function useCornersAway(
  box: RefObject<HTMLElement | null>,
  left: RefObject<HTMLElement | null>,
  right: RefObject<HTMLElement | null>,
  pillWidth: number | null,
): boolean {
  const [away, setAway] = useState(false);
  useLayoutEffect(() => {
    const frame = box.current;
    const lines = [left.current, right.current];
    if (!frame || pillWidth === null || lines.includes(null)) {
      setAway(false);
      return;
    }
    const measure = () => {
      const free = (frame.clientWidth - pillWidth) / 2 - CORNER_SIDE_PX - PILL_CLEARANCE_PX;
      setAway(Math.max(...lines.map((line) => line!.offsetWidth)) > free);
    };
    measure();
    const observer = new ResizeObserver(measure);
    for (const el of [frame, ...lines]) observer.observe(el!);
    return () => observer.disconnect();
  }, [box, left, right, pillWidth]);
  return away;
}

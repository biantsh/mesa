import { isMac } from "./platform";
import { prefersReducedMotion } from "./motion";
import { scrollSmoothly } from "./scrollKeys";

const LINE_PX = 40;

export function smoothMouseWheel() {
  if (!isMac) return;
  window.addEventListener("wheel", onWheel, { passive: false, capture: true });
}

function onWheel(e: WheelEvent) {
  if (e.defaultPrevented || e.ctrlKey || (e.deltaMode === 0 && !isNotch(e))) return;
  if (Math.abs(e.deltaY) <= Math.abs(e.deltaX) || prefersReducedMotion()) return;
  if (document.querySelector("[data-marquee='on']")) return;

  const view = scrollingViewFrom(e.target, e.deltaY);
  if (!view) return;
  e.preventDefault();
  const unit = e.deltaMode === 1 ? LINE_PX : e.deltaMode === 2 ? view.clientHeight : 1;
  scrollSmoothly(view, e.deltaY * unit);
}

function isNotch(e: WheelEvent) {
  const legacy = (e as WheelEvent & { wheelDeltaY?: number }).wheelDeltaY;
  return legacy !== undefined && legacy !== 0 && legacy % 120 === 0;
}

function scrollingViewFrom(from: EventTarget | null, deltaY: number): HTMLElement | null {
  for (let el = from instanceof Element ? from : null; el; el = el.parentElement) {
    if (!(el instanceof HTMLElement)) continue;
    const overflow = getComputedStyle(el).overflowY;
    if ((overflow !== "auto" && overflow !== "scroll") || el.scrollHeight <= el.clientHeight) continue;
    const atTop = el.scrollTop <= 0;
    const atBottom = el.scrollTop >= el.scrollHeight - el.clientHeight - 1;
    if (deltaY < 0 ? !atTop : !atBottom) return el;
  }
  return null;
}

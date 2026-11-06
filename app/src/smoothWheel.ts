import { isMac } from "./platform";
import { prefersReducedMotion } from "./motion";
import { scrollSmoothly } from "./scrollKeys";

const LINE_PX = 40;

export function smoothMouseWheel() {
  if (!isMac) return;
  window.addEventListener("wheel", onWheel, { passive: false, capture: true });
}

const RUN_MS = 250;
let lastScrolledAt = -Infinity;

function onWheel(e: WheelEvent) {
  const legacy = (e as WheelEvent & { wheelDeltaY?: number }).wheelDeltaY;
  if (e.defaultPrevented || e.ctrlKey || !isWheelTurn(e.deltaMode, legacy, performance.now() - lastScrolledAt)) return;
  if (Math.abs(e.deltaY) <= Math.abs(e.deltaX) || prefersReducedMotion()) return;
  if (document.querySelector("[data-marquee='on']")) return;

  const view = scrollingViewFrom(e.target, e.deltaY);
  if (!view) return;
  e.preventDefault();
  lastScrolledAt = performance.now();
  const unit = e.deltaMode === 1 ? LINE_PX : e.deltaMode === 2 ? view.clientHeight : 1;
  scrollSmoothly(view, e.deltaY * unit);
}

export function isWheelTurn(deltaMode: number, wheelDeltaY: number | undefined, sinceLast: number): boolean {
  if (deltaMode !== 0) return true;
  if (wheelDeltaY !== undefined && wheelDeltaY !== 0 && wheelDeltaY % 120 === 0) return true;
  return sinceLast < RUN_MS;
}

function scrollingViewFrom(from: EventTarget | null, deltaY: number): HTMLElement | null {
  for (let el = from instanceof Element ? from : null; el; el = el.parentElement) {
    if (!(el instanceof HTMLElement) || el.scrollHeight <= el.clientHeight) continue;
    const overflow = getComputedStyle(el).overflowY;
    if (overflow !== "auto" && overflow !== "scroll") continue;
    const atTop = el.scrollTop <= 0;
    const atBottom = el.scrollTop >= el.scrollHeight - el.clientHeight - 1;
    if (deltaY < 0 ? !atTop : !atBottom) return el;
  }
  return null;
}

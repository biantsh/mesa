import { platform } from "./platform";

const BOX_MARGIN_PX = 2;
const LOOK_FOR_MS = 500;
const SETTLE_MS = 150;

export type Box = [number, number, number, number];

export function visibleBox(): Box | null {
  const backdrop = document.querySelector<HTMLElement>("[data-widget-backdrop]");
  const spot = document.querySelector<HTMLElement>("[data-widget-spot]");
  const seen = backdrop && Number(getComputedStyle(backdrop).opacity) > 0.01 ? backdrop : spot;
  if (!seen) return null;
  let r = { left: 0, top: 0, right: 0, bottom: 0, ...seen.getBoundingClientRect().toJSON() };
  if (document.documentElement.dataset.widgetFill !== undefined) {
    for (const popup of Array.from(document.querySelectorAll<HTMLElement>("[data-popup]"))) {
      const p = popup.getBoundingClientRect();
      r = { left: Math.min(r.left, p.left), top: Math.min(r.top, p.top), right: Math.max(r.right, p.right), bottom: Math.max(r.bottom, p.bottom) };
    }
  }
  return [r.left - BOX_MARGIN_PX, r.top - BOX_MARGIN_PX, r.right + BOX_MARGIN_PX, r.bottom + BOX_MARGIN_PX].map(Math.round) as Box;
}

export function reportVisibleBox(say: (box: Box) => void): () => void {
  let frame = 0;
  let until = 0;
  let said = "";

  const look = () => {
    frame = 0;
    const now = performance.now();
    const box = visibleBox();
    const text = box?.join(",") ?? "";
    if (box && text !== said) {
      said = text;
      say(box);
      until = Math.max(until, now + SETTLE_MS);
    }
    if (now < until) frame = requestAnimationFrame(look);
  };

  const wake = () => {
    until = Math.max(until, performance.now() + LOOK_FOR_MS);
    if (!frame) frame = requestAnimationFrame(look);
  };

  wake();
  document.addEventListener("transitionrun", wake, true);
  document.addEventListener("transitionend", wake, true);
  document.addEventListener("transitioncancel", wake, true);
  window.addEventListener("resize", wake);
  const changes = new MutationObserver(wake);
  changes.observe(document.body, { subtree: true, childList: platform === "linux", attributes: true, attributeFilter: ["class", "style"] });

  return () => {
    cancelAnimationFrame(frame);
    document.removeEventListener("transitionrun", wake, true);
    document.removeEventListener("transitionend", wake, true);
    document.removeEventListener("transitioncancel", wake, true);
    window.removeEventListener("resize", wake);
    changes.disconnect();
  };
}

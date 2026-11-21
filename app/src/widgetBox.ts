import { platform } from "./platform";

const BOX_MARGIN_PX = 2;
const LOOK_FOR_MS = 500;
const SETTLE_MS = 150;

export type Box = [number, number, number, number];

function padded(r: DOMRect): Box {
  return [r.left - BOX_MARGIN_PX, r.top - BOX_MARGIN_PX, r.right + BOX_MARGIN_PX, r.bottom + BOX_MARGIN_PX].map(Math.round) as Box;
}

export function visibleBoxes(): Box[] | null {
  const backdrop = document.querySelector<HTMLElement>("[data-widget-backdrop]");
  const spot = document.querySelector<HTMLElement>("[data-widget-spot]");
  const seen = backdrop && Number(getComputedStyle(backdrop).opacity) > 0.01 ? backdrop : spot;
  if (!seen) return null;
  const popups = Array.from(document.querySelectorAll<HTMLElement>("[data-popup]"))
    .map((popup) => popup.getBoundingClientRect())
    .filter((r) => r.width > 0 && r.height > 0);
  return [seen.getBoundingClientRect(), ...popups].map(padded);
}

export function reportVisibleBoxes(say: (boxes: Box[]) => void): () => void {
  let frame = 0;
  let until = 0;
  let said = "";

  const look = () => {
    frame = 0;
    const now = performance.now();
    const boxes = visibleBoxes();
    const text = boxes?.join(";") ?? "";
    if (boxes && text !== said) {
      said = text;
      say(boxes);
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
  const popups = new MutationObserver(wake);
  popups.observe(document.body, { childList: true });

  return () => {
    cancelAnimationFrame(frame);
    document.removeEventListener("transitionrun", wake, true);
    document.removeEventListener("transitionend", wake, true);
    document.removeEventListener("transitioncancel", wake, true);
    window.removeEventListener("resize", wake);
    changes.disconnect();
    popups.disconnect();
  };
}

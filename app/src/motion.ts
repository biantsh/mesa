import { isWebKitGTK } from "./platform";

export const prefersReducedMotion = () => matchMedia("(prefers-reduced-motion: reduce)").matches;

export interface Look {
  x?: number;
  y?: number;
  scaleX?: number;
  scaleY?: number;
  opacity?: number;
}

const STILL = { x: 0, y: 0, scaleX: 1, scaleY: 1, opacity: 1 };

function keyframe(look: Look, fading: boolean): Keyframe {
  const at = { ...STILL, ...look };
  const frame: Keyframe = { transform: `translate(${at.x}px, ${at.y}px) scale(${at.scaleX}, ${at.scaleY})` };
  if (fading) frame.opacity = at.opacity;
  return frame;
}

let moveRegistered = false;
const moving = new WeakMap<HTMLElement, { animation: Animation; own: { transform: string; opacity: string } }>();

// Moves and fades a large surface. On WebKitGTK it runs on the main thread: a registered --move goes from 0 to 1 and
// the transform and opacity follow it.
export function animateLarge(element: HTMLElement, from: Look, to: Look, options: KeyframeAnimationOptions): Animation {
  const fading = from.opacity !== undefined || to.opacity !== undefined;
  if (!isWebKitGTK) return element.animate([keyframe(from, fading), keyframe(to, fading)], options);
  if (!moveRegistered) {
    try {
      CSS.registerProperty({ name: "--move", syntax: "<number>", inherits: false, initialValue: "0" });
    } catch {}
    moveRegistered = true;
  }
  const a = { ...STILL, ...from };
  const b = { ...STILL, ...to };
  const between = (p: number, q: number) => `calc(${p} + ${q - p} * var(--move))`;
  // A motion started over a running one takes over: it keeps the style from before the first, and only the newest
  // puts it back. Otherwise the second would restore the first's starting place for good.
  const before = moving.get(element);
  const own = before?.own ?? { transform: element.style.transform, opacity: element.style.opacity };
  element.style.transform = `translate(calc(${between(a.x, b.x)} * 1px), calc(${between(a.y, b.y)} * 1px)) scale(${between(a.scaleX, b.scaleX)}, ${between(a.scaleY, b.scaleY)})`;
  element.style.opacity = fading ? between(a.opacity, b.opacity) : own.opacity;
  const animation = element.animate([{ "--move": 0 }, { "--move": 1 }], options);
  moving.set(element, { animation, own });
  before?.animation.cancel();
  const restore = () => {
    if (moving.get(element)?.animation !== animation) return;
    moving.delete(element);
    element.style.transform = own.transform;
    element.style.opacity = own.opacity;
  };
  if (options.fill !== "forwards" && options.fill !== "both") animation.addEventListener("finish", restore);
  animation.addEventListener("cancel", restore);
  return animation;
}

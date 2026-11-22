import { flushSync } from "react-dom";
import { animateLarge, prefersReducedMotion } from "./motion";
import { isWebKitGTK } from "./platform";

type Start = (update: () => void) => { finished: Promise<unknown> };

// Matches ::view-transition-group(index-switch) in global.css.
const GLIDE_MS = 420;
const GLIDE_EASE = "cubic-bezier(0.22, 0.61, 0.36, 1)";
// Above anything else on a screen; dialogs and tooltips are outside it.
const GLIDE_LAYER = "100";
const gliding = new WeakMap<HTMLElement, { motion: Animation; own: { position: string; zIndex: string; willChange: string } }>();

export function changeWithTransition(change: () => void, { onlyMorph = false } = {}) {
  const start = (document as Document & { startViewTransition?: Start }).startViewTransition;
  if (!start || prefersReducedMotion()) {
    change();
    return;
  }
  if (isWebKitGTK) {
    glide(change);
    return;
  }
  const root = document.documentElement;
  if (onlyMorph) root.dataset.morphOnly = "";
  const transition = start.call(document, () => flushSync(change));
  const over = () => delete root.dataset.morphOnly;
  transition.finished.then(over, over);
}

// WebKitGTK can show the whole window transparent for a frame of a view transition, so there the change happens at
// once and only the named elements glide over from where their namesakes were, above the rest of the page.
function glide(change: () => void) {
  const before = new Map([...named()].map(([name, element]) => [name, element.getBoundingClientRect()]));
  flushSync(change);
  for (const [name, element] of named()) {
    const from = before.get(name);
    if (!from) continue;
    const to = element.getBoundingClientRect();
    const x = from.left + from.width / 2 - (to.left + to.width / 2);
    const y = from.top + from.height / 2 - (to.top + to.height / 2);
    if (Math.abs(x) < 1 && Math.abs(y) < 1) continue;
    // As in animateLarge, a glide over a running one keeps the style from before the first.
    const own = gliding.get(element)?.own ?? { position: element.style.position, zIndex: element.style.zIndex, willChange: element.style.willChange };
    if (getComputedStyle(element).position === "static") element.style.position = "relative";
    element.style.zIndex = GLIDE_LAYER;
    // Its own layer while it moves; repainting the page under it each frame leaves streaks on WebKitGTK.
    element.style.willChange = "transform";
    const motion = animateLarge(element, { x, y }, {}, { duration: GLIDE_MS, easing: GLIDE_EASE });
    gliding.set(element, { motion, own });
    const restore = () => {
      if (gliding.get(element)?.motion !== motion) return;
      gliding.delete(element);
      Object.assign(element.style, own);
    };
    motion.finished.then(restore, restore);
  }
}

function named(): Map<string, HTMLElement> {
  const found = new Map<string, HTMLElement>();
  for (const element of Array.from(document.querySelectorAll<HTMLElement>("[style*='view-transition-name']"))) {
    const name = element.style.getPropertyValue("view-transition-name");
    if (name && name !== "none") found.set(name, element);
  }
  return found;
}

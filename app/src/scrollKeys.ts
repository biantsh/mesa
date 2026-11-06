import { prefersReducedMotion as reducedMotion } from "./motion";

const LINE = 48;
const HELD_SPEED = 1100;
const LET_GO_MS = 120;

const sent = new WeakMap<HTMLElement, { top: number; at: number }>();

const GOING_MS = 350;

let held: { view: HTMLElement; direction: 1 | -1; lastRepeat: number; frame: number } | null = null;

export function scrollByKey(view: HTMLElement, key: string, repeat = false): boolean {
  const page = view.clientHeight * 0.9;
  const delta: Record<string, number> = { ArrowDown: LINE, ArrowUp: -LINE, PageDown: page, PageUp: -page };
  const end = view.scrollHeight - view.clientHeight;

  if (repeat && (key === "ArrowDown" || key === "ArrowUp") && !reducedMotion()) {
    hold(view, key === "ArrowDown" ? 1 : -1);
    return true;
  }

  if (key in delta) scrollSmoothly(view, delta[key]);
  else if (key === "Home") scrollTowards(view, 0);
  else if (key === "End") scrollTowards(view, end);
  else return false;
  return true;
}

function scrollTowards(view: HTMLElement, top: number) {
  const to = Math.max(0, Math.min(view.scrollHeight - view.clientHeight, top));
  sent.set(view, { top: to, at: performance.now() });
  view.scrollTo({ top: to, behavior: reducedMotion() ? "auto" : "smooth" });
}

export function scrollSmoothly(view: HTMLElement, delta: number) {
  const last = sent.get(view);
  const going = last !== undefined && performance.now() - last.at < GOING_MS;
  scrollTowards(view, (going ? last.top : view.scrollTop) + delta);
}

function hold(view: HTMLElement, direction: 1 | -1) {
  const now = performance.now();
  if (held && held.view === view && held.direction === direction) {
    held.lastRepeat = now;
    return;
  }
  if (held) cancelAnimationFrame(held.frame);
  view.scrollTo({ top: view.scrollTop, behavior: "auto" });
  sent.delete(view);

  const state = { view, direction, lastRepeat: now, frame: 0 };
  held = state;
  let before = now;
  const tick = (time: number) => {
    if (held !== state) return;
    if (time - state.lastRepeat > LET_GO_MS) {
      held = null;
      return;
    }
    const step = Math.max(0, time - before);
    before = time;
    view.scrollTop += direction * HELD_SPEED * (step / 1000);
    state.frame = requestAnimationFrame(tick);
  };
  state.frame = requestAnimationFrame(tick);
}

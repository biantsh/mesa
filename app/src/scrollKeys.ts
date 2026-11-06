import { prefersReducedMotion as reducedMotion } from "./motion";

const LINE = 48;
const HELD_SPEED = 1100;
const LET_GO_MS = 120;
const GLIDE_MS = 70;
const MOVED_BY_OTHERS_PX = 1.5;

interface Glide {
  to: number;
  at: number;
  seen: number;
  time: number;
  frame: number;
}

const gliding = new WeakMap<HTMLElement, Glide>();

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
  else if (key === "Home") glideTo(view, 0);
  else if (key === "End") glideTo(view, end);
  else return false;
  return true;
}

export function scrollSmoothly(view: HTMLElement, delta: number) {
  const going = gliding.get(view);
  glideTo(view, (going ? going.to : view.scrollTop) + delta);
}

function glideTo(view: HTMLElement, top: number) {
  const to = Math.max(0, Math.min(view.scrollHeight - view.clientHeight, top));
  if (reducedMotion()) {
    stopGlide(view);
    view.scrollTop = to;
    return;
  }
  const going = gliding.get(view);
  if (going) {
    going.to = to;
    return;
  }
  const glide: Glide = { to, at: view.scrollTop, seen: view.scrollTop, time: performance.now(), frame: 0 };
  gliding.set(view, glide);
  const tick = (now: number) => {
    if (Math.abs(view.scrollTop - glide.seen) > MOVED_BY_OTHERS_PX) {
      gliding.delete(view);
      return;
    }
    const step = Math.max(0, now - glide.time);
    glide.time = now;
    glide.at += (glide.to - glide.at) * (1 - Math.exp(-step / GLIDE_MS));
    const arrived = Math.abs(glide.to - glide.at) < 0.5;
    if (arrived) glide.at = glide.to;
    view.scrollTop = glide.at;
    glide.seen = view.scrollTop;
    if (arrived) gliding.delete(view);
    else glide.frame = requestAnimationFrame(tick);
  };
  glide.frame = requestAnimationFrame(tick);
}

function stopGlide(view: HTMLElement) {
  const going = gliding.get(view);
  if (!going) return;
  cancelAnimationFrame(going.frame);
  gliding.delete(view);
}

function hold(view: HTMLElement, direction: 1 | -1) {
  const now = performance.now();
  if (held && held.view === view && held.direction === direction) {
    held.lastRepeat = now;
    return;
  }
  if (held) cancelAnimationFrame(held.frame);
  stopGlide(view);

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

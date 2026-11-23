import "./styles/tooltip.css";
import { prefersReducedMotion } from "./motion";
import { isResizing } from "./resizing";

const SHOW_DELAY_MS = 500;
const WARM_MS = 400;
const FADE_OUT_MS = 80;
const MARGIN_PX = 8;
const GAP_PX = 8;
const BESIDE_GAP_PX = 24;
const TALL_PX = 60;
const NARROW_PX = 120;

export function installTooltips() {
  const tip = document.createElement("div");
  tip.className = "mesa-tooltip";
  tip.id = "mesa-tooltip";
  tip.setAttribute("role", "tooltip");
  const popover = typeof tip.showPopover === "function";
  if (popover) tip.setAttribute("popover", "manual");
  document.body.appendChild(tip);

  let anchor: HTMLElement | null = null;
  let quiet = false;
  let shown = false;
  let timer = 0;
  let watch = 0;
  let hiddenAt = -Infinity;
  let pointer = { x: 0, y: 0 };
  let describedBefore: string | null = null;
  let leaving = 0;

  const takeDown = () => {
    window.clearTimeout(leaving);
    leaving = 0;
    delete tip.dataset.shown;
    delete tip.dataset.leaving;
    delete tip.dataset.instant;
    if (popover) {
      try {
        tip.hidePopover();
      } catch {
      }
    } else {
      tip.style.display = "none";
    }
  };

  const hide = (fade: boolean) => {
    window.clearTimeout(timer);
    window.clearInterval(watch);
    if (!shown) return;
    shown = false;
    hiddenAt = performance.now();
    if (anchor) {
      if (describedBefore === null) anchor.removeAttribute("aria-describedby");
      else anchor.setAttribute("aria-describedby", describedBefore);
    }
    if (fade && !prefersReducedMotion()) {
      tip.dataset.leaving = "";
      leaving = window.setTimeout(takeDown, FADE_OUT_MS);
    } else {
      takeDown();
    }
  };

  const show = (instant: boolean) => {
    const target = anchor;
    const text = target?.dataset.tooltip?.trim();
    if (!target || !target.isConnected || !text || quiet) return;
    if (leaving) takeDown();
    tip.textContent = text;
    tip.dataset.instant = "";
    if (popover) {
      try {
        tip.showPopover();
      } catch {
        return;
      }
    } else {
      (target.closest("dialog[open]") ?? document.body).appendChild(tip);
      tip.style.display = "block";
    }
    tip.dataset.side = place(tip, target, pointer);
    void tip.offsetWidth;
    shown = true;
    describedBefore = target.getAttribute("aria-describedby");
    if (target.getAttribute("aria-label") !== text) {
      target.setAttribute("aria-describedby", describedBefore ? `${describedBefore} ${tip.id}` : tip.id);
    }
    if (instant) {
      tip.dataset.shown = "";
    } else {
      delete tip.dataset.instant;
      requestAnimationFrame(() => {
        if (shown) tip.dataset.shown = "";
      });
    }
    watch = window.setInterval(() => {
      if (!target.isConnected) {
        hide(false);
        anchor = null;
        return;
      }
      const now = target.dataset.tooltip?.trim();
      if (now && now !== tip.textContent) {
        tip.textContent = now;
        tip.dataset.side = place(tip, target, pointer);
      }
    }, 250);
  };

  let looking = 0;
  const onMove = (e: PointerEvent) => {
    if (e.pointerType === "touch") return;
    pointer = { x: e.clientX, y: e.clientY };
    if (!looking) looking = requestAnimationFrame(look);
  };

  const look = () => {
    looking = 0;
    if (isResizing()) {
      hide(false);
      anchor = null;
      quiet = false;
      return;
    }
    const under = document.elementFromPoint(pointer.x, pointer.y);
    const next = under instanceof Element ? under.closest<HTMLElement>("[data-tooltip]") : null;
    if (next === anchor) return;
    const wasShown = shown;
    hide(next === null);
    anchor = next;
    quiet = false;
    if (!next) return;
    if (wasShown) show(true);
    else if (performance.now() - hiddenAt < WARM_MS) timer = window.setTimeout(() => show(true), 0);
    else timer = window.setTimeout(() => show(false), SHOW_DELAY_MS);
  };

  const done = () => {
    hide(false);
    if (anchor) quiet = true;
  };
  const gone = (fade: boolean) => {
    cancelAnimationFrame(looking);
    looking = 0;
    hide(fade);
    anchor = null;
    quiet = false;
  };

  window.addEventListener("pointermove", onMove, { capture: true, passive: true });
  window.addEventListener("pointerdown", done, { capture: true, passive: true });
  window.addEventListener("keydown", done, { capture: true, passive: true });
  window.addEventListener("wheel", done, { capture: true, passive: true });
  window.addEventListener("scroll", done, { capture: true, passive: true });
  window.addEventListener("blur", () => gone(false));
  document.addEventListener("pointerleave", () => gone(true));
  document.addEventListener("visibilitychange", () => gone(false));
}

function seenBox(target: HTMLElement): { left: number; right: number; top: number; bottom: number; width: number; height: number } {
  const box = target.getBoundingClientRect();
  const style = getComputedStyle(target);
  const unseen = (side: "Top" | "Right" | "Bottom" | "Left") =>
    style.getPropertyValue(`border-${side.toLowerCase()}-color`) === "rgba(0, 0, 0, 0)"
      ? parseFloat(style.getPropertyValue(`border-${side.toLowerCase()}-width`)) || 0
      : 0;
  const left = box.left + unseen("Left");
  const right = box.right - unseen("Right");
  const top = box.top + unseen("Top");
  const bottom = box.bottom - unseen("Bottom");
  return { left, right, top, bottom, width: right - left, height: bottom - top };
}

function place(tip: HTMLElement, target: HTMLElement, pointer: { x: number; y: number }): "below" | "above" | "right" | "left" {
  const box = seenBox(target);
  const width = tip.offsetWidth;
  const height = tip.offsetHeight;
  if (target.getAttribute("role") === "separator" && target.getAttribute("aria-orientation") === "vertical") {
    const right = box.right + BESIDE_GAP_PX;
    const onRight = right + width <= window.innerWidth - MARGIN_PX;
    const left = onRight ? right : Math.max(MARGIN_PX, box.left - BESIDE_GAP_PX - width);
    const top = Math.min(Math.max(MARGIN_PX, pointer.y - height / 2), window.innerHeight - MARGIN_PX - height);
    tip.style.left = `${Math.round(left)}px`;
    tip.style.top = `${Math.round(top)}px`;
    tip.style.removeProperty("--tip-origin");
    return onRight ? "right" : "left";
  }
  const x = box.width < NARROW_PX ? box.left + box.width / 2 : Math.min(Math.max(pointer.x, box.left), box.right);
  const tall = box.height > TALL_PX;
  const below = (tall ? pointer.y + 18 : box.bottom) + GAP_PX;
  const above = (tall ? pointer.y - 6 : box.top) - GAP_PX - height;
  const fits = below + height <= window.innerHeight - MARGIN_PX;
  const top = fits ? below : Math.max(MARGIN_PX, above);
  const left = Math.min(Math.max(MARGIN_PX, x - width / 2), window.innerWidth - MARGIN_PX - width);
  tip.style.left = `${Math.round(left)}px`;
  tip.style.top = `${Math.round(top)}px`;
  tip.style.setProperty("--tip-origin", `${Math.round(Math.min(Math.max(x - left, 0), width))}px`);
  return fits ? "below" : "above";
}

import { useCallback, useEffect, useRef, type PointerEvent as ReactPointerEvent } from "react";
import { areaOf, boxBetween, intersect, touching, type Box, type Placed } from "./marquee";
import { scrollSmoothly } from "./scrollKeys";
import { useLatest } from "./useLatest";
import { virtualViews } from "./virtual";

export interface MarqueeHandlers {
  begin: (additive: boolean) => void;
  sweep: (paths: string[]) => void;
  cancel: () => void;
}

interface Options {
  regions: Record<string, MarqueeHandlers>;
  onBackground: () => void;
}

interface Region {
  view: HTMLElement;
  handlers: MarqueeHandlers;
}

const THRESHOLD = 4;
const EDGE = 28;
const MAX_SPEED = 18;

function viewport(view: HTMLElement): Box {
  const r = view.getBoundingClientRect();
  const left = r.left + view.clientLeft;
  const top = r.top + view.clientTop;
  return { left, top, right: left + view.clientWidth, bottom: top + view.clientHeight };
}

function regionsIn(page: HTMLElement, handlers: Record<string, MarqueeHandlers>): Region[] {
  return Array.from(page.querySelectorAll<HTMLElement>("[data-region]")).flatMap((view) => {
    const mine = handlers[view.dataset.region ?? ""];
    const shown = !view.closest("[inert]") && view.clientHeight > 0 && getComputedStyle(view).visibility !== "hidden";
    return mine && shown ? [{ view, handlers: mine }] : [];
  });
}

export function useMarquee({ regions, onBackground }: Options) {
  const root = useRef<HTMLDivElement>(null);
  const box = useRef<HTMLDivElement>(null);

  const latest = useLatest({ regions, onBackground });
  const dragging = useRef(false);
  const stop = useRef<(() => void) | null>(null);
  useEffect(() => () => stop.current?.(), []);

  const onPointerDown = useCallback((e: ReactPointerEvent<HTMLElement>) => {
    const page = root.current;
    if (!page || e.button !== 0 || e.pointerType !== "mouse") return;
    const target = e.target as Element;
    if (target.closest("dialog, [role='menu'], [data-no-marquee]")) return;
    const own = target.closest("button, [role='button'], a, input, select, textarea");
    if (own && !own.matches("[data-path]")) return;

    const all = regionsIn(page, latest.current.regions);
    for (const { view } of all) {
      if (target !== view) continue;
      const seen = viewport(view);
      if (e.clientX >= seen.right || e.clientY >= seen.bottom) return;
    }

    const additive = e.ctrlKey || e.metaKey;
    const pressed = { x: e.clientX, y: e.clientY };
    const home = all.find(({ view }) => view.contains(target)) ?? null;

    let last = pressed;
    let phase: "pressed" | "dragging" | "cancelled" = "pressed";
    let locked: Region | null = null;
    let items: Placed[] = [];
    let corner = { x: 0, y: 0 };
    let frame = 0;
    let shown: string | null = null;

    const originOf = (view: HTMLElement) => {
      const seen = viewport(view);
      return { x: seen.left - view.scrollLeft, y: seen.top - view.scrollTop };
    };

    const place = (view: HTMLElement): Placed[] => {
      const virtual = virtualViews.get(view);
      if (virtual) return virtual.place();
      const origin = originOf(view);
      return Array.from(view.querySelectorAll<HTMLElement>("[data-path]"))
        .filter((el) => el.getAttribute("aria-disabled") !== "true")
        .map((el) => {
          const b = el.getBoundingClientRect();
          return {
            path: el.dataset.path ?? "",
            left: b.left - origin.x,
            top: b.top - origin.y,
            right: b.right - origin.x,
            bottom: b.bottom - origin.y,
          };
        });
    };

    const lock = (region: Region) => {
      locked = region;
      const origin = originOf(region.view);
      corner = { x: pressed.x - origin.x, y: pressed.y - origin.y };
      items = place(region.view);
      region.handlers.begin(additive);
      region.view.closest<HTMLElement>("[tabindex='-1']")?.focus({ preventScroll: true });
    };

    const draw = (area: Box | null) => {
      const rectangle = box.current;
      if (!rectangle) return;
      if (!area) {
        rectangle.style.display = "none";
        return;
      }
      const p = root.current?.getBoundingClientRect();
      rectangle.style.display = "block";
      rectangle.style.left = `${area.left - (p?.left ?? 0)}px`;
      rectangle.style.top = `${area.top - (p?.top ?? 0)}px`;
      rectangle.style.width = `${area.right - area.left}px`;
      rectangle.style.height = `${area.bottom - area.top}px`;
    };

    const tick = () => {
      if (phase !== "dragging") return;
      const current = locked as Region | null;

      if (current) {
        const seen = viewport(current.view);
        const above = EDGE - (last.y - seen.top);
        const below = EDGE - (seen.bottom - last.y);
        if (above > 0) current.view.scrollTop -= MAX_SPEED * Math.min(1, above / EDGE);
        else if (below > 0) current.view.scrollTop += MAX_SPEED * Math.min(1, below / EDGE);
      }

      let from = pressed;
      if (current && home === current) {
        const origin = originOf(current.view);
        const seen = viewport(current.view);
        from = {
          x: Math.max(seen.left, Math.min(seen.right, corner.x + origin.x)),
          y: Math.max(seen.top, Math.min(seen.bottom, corner.y + origin.y)),
        };
      }
      const whole = boxBetween(from, last);

      if (!current) {
        let best: Region | null = null;
        let most = -1;
        for (const region of regionsIn(root.current as HTMLElement, latest.current.regions)) {
          const meeting = intersect(whole, viewport(region.view));
          if (meeting && areaOf(meeting) > most) {
            best = region;
            most = areaOf(meeting);
          }
        }
        if (best) lock(best);
      }

      const now = locked as Region | null;
      draw(whole);
      if (now) {
        const seen = viewport(now.view);
        const o = originOf(now.view);
        const pointer = {
          x: Math.max(seen.left, Math.min(seen.right, last.x)) - o.x,
          y: Math.max(seen.top, Math.min(seen.bottom, last.y)) - o.y,
        };
        const paths = touching(items, boxBetween(corner, pointer));
        const key = paths.join("\n");
        if (key !== shown) {
          shown = key;
          now.handlers.sweep(paths);
        }
      }
      frame = requestAnimationFrame(tick);
    };

    const onMove = (ev: PointerEvent) => {
      last = { x: ev.clientX, y: ev.clientY };
      if (phase !== "pressed" || Math.hypot(last.x - pressed.x, last.y - pressed.y) <= THRESHOLD) return;
      phase = "dragging";
      dragging.current = true;
      page.dataset.marquee = "on";
      page.style.userSelect = "none";
      window.getSelection()?.removeAllRanges();
      window.addEventListener("wheel", onWheel, { passive: false });
      if (home) lock(home);
      frame = requestAnimationFrame(tick);
    };

    const onWheel = (ev: WheelEvent) => {
      const region = locked as Region | null;
      if (!region || (ev.target instanceof Node && region.view.contains(ev.target))) return;
      ev.preventDefault();
      const unit = ev.deltaMode === 1 ? 40 : ev.deltaMode === 2 ? region.view.clientHeight : 1;
      scrollSmoothly(region.view, ev.deltaY * unit);
    };

    const hide = () => {
      draw(null);
      dragging.current = false;
      delete page.dataset.marquee;
      page.style.userSelect = "";
    };

    const onKey = (ev: KeyboardEvent) => {
      if (ev.key !== "Escape" || phase !== "dragging") return;
      ev.preventDefault();
      ev.stopPropagation();
      phase = "cancelled";
      cancelAnimationFrame(frame);
      hide();
      (locked as Region | null)?.handlers.cancel();
    };

    const finish = () => {
      window.removeEventListener("pointermove", onMove);
      window.removeEventListener("pointerup", finish);
      window.removeEventListener("pointercancel", finish);
      window.removeEventListener("blur", finish);
      window.removeEventListener("keydown", onKey, true);
      window.removeEventListener("wheel", onWheel);
      cancelAnimationFrame(frame);
      stop.current = null;
      if (phase === "pressed") return;
      if (phase === "dragging" && !locked && !additive) latest.current.onBackground();
      hide();
      const swallow = (ev: Event) => {
        ev.stopPropagation();
        ev.preventDefault();
      };
      window.addEventListener("click", swallow, true);
      setTimeout(() => window.removeEventListener("click", swallow, true), 0);
    };

    window.addEventListener("pointermove", onMove);
    window.addEventListener("pointerup", finish);
    window.addEventListener("pointercancel", finish);
    window.addEventListener("blur", finish);
    window.addEventListener("keydown", onKey, true);
    stop.current = finish;
  }, [latest]);

  return { root, box, dragging, onPointerDown };
}

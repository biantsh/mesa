import type { KeyboardEvent } from "react";
import { placesOf } from "./entriesShape";
import { NAV_KEYS, navigateAmong, type NavKey } from "./navigation";
import { scrollByKey } from "./scrollKeys";
import { virtualViews } from "./virtual";

interface Options<T> {
  items: readonly T[];
  selectable: (item: T) => boolean;
  active: string | null;
  columns: (container: HTMLElement) => number;
  onMove: (path: string, extend: boolean) => void;
}

export function focusItem(container: HTMLElement, path: string) {
  const find = () => Array.from(container.querySelectorAll<HTMLElement>("[data-path]")).find((item) => item.dataset.path === path);
  const item = find();
  if (item) return item.focus();
  const scroller = container.matches("[data-scroller]") ? container : container.querySelector<HTMLElement>("[data-scroller]");
  const virtual = scroller && virtualViews.get(scroller);
  if (!virtual) return;
  virtual.reveal(path);
  let tries = 3;
  const again = () => {
    const made = find();
    if (made) made.focus();
    else if (--tries > 0) requestAnimationFrame(again);
  };
  requestAnimationFrame(again);
}

export function useArrowKeys<T extends { path: string }>({ items, selectable, active, columns, onMove }: Options<T>) {
  return (e: KeyboardEvent<HTMLElement>) => {
    if (e.ctrlKey || e.altKey || e.metaKey || !NAV_KEYS.has(e.key)) return;
    const container = e.currentTarget;
    const wide = columns(container) > 1;
    if (!wide && (e.key === "ArrowLeft" || e.key === "ArrowRight")) return;

    const focused = e.target instanceof HTMLElement ? e.target.closest<HTMLElement>("[data-path]")?.dataset.path : null;
    const places = placesOf(items);
    const known = (path: string | null | undefined) => (path != null && places.has(path) ? path : null);
    const fromPath = known(active) ?? known(focused);
    if (fromPath === null) {
      const view = container.matches("[data-scroller]") ? container : container.querySelector<HTMLElement>("[data-scroller]");
      if (view && scrollByKey(view, e.key, e.repeat)) e.preventDefault();
      return;
    }
    e.preventDefault();
    const from = places.get(fromPath) ?? -1;

    const to = navigateAmong(items.length, (at) => selectable(items[at]), from, e.key as NavKey, columns(container));
    if (to === null) return;
    const path = items[to].path;
    onMove(path, e.shiftKey);
    focusItem(container, path);
  };
}

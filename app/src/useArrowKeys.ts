import type { KeyboardEvent } from "react";
import { NAV_KEYS, navigate, type NavKey } from "./navigation";
import { scrollByKey } from "./scrollKeys";

interface Options<T> {
  items: readonly T[];
  selectable: (item: T) => boolean;
  active: string | null;
  columns: (container: HTMLElement) => number;
  onMove: (path: string, extend: boolean) => void;
}

export function focusItem(container: HTMLElement, path: string) {
  const items = Array.from(container.querySelectorAll<HTMLElement>("[data-path]"));
  items.find((item) => item.dataset.path === path)?.focus();
}

export function useArrowKeys<T extends { path: string }>({ items, selectable, active, columns, onMove }: Options<T>) {
  return (e: KeyboardEvent<HTMLElement>) => {
    if (e.ctrlKey || e.altKey || e.metaKey || !NAV_KEYS.has(e.key)) return;
    const container = e.currentTarget;
    const wide = columns(container) > 1;
    if (!wide && (e.key === "ArrowLeft" || e.key === "ArrowRight")) return;

    const focused = e.target instanceof HTMLElement ? e.target.closest<HTMLElement>("[data-path]")?.dataset.path : null;
    const known = (path: string | null | undefined) => (path != null && items.some((item) => item.path === path) ? path : null);
    const fromPath = known(active) ?? known(focused);
    if (fromPath === null) {
      const view = container.matches("[data-scroller]") ? container : container.querySelector<HTMLElement>("[data-scroller]");
      if (view && scrollByKey(view, e.key, e.repeat)) e.preventDefault();
      return;
    }
    e.preventDefault();
    const from = items.findIndex((item) => item.path === fromPath);

    const to = navigate(items.map(selectable), from, e.key as NavKey, columns(container));
    if (to === null) return;
    const path = items[to].path;
    onMove(path, e.shiftKey);
    focusItem(container, path);
  };
}

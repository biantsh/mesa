import type { Entry } from "../api";
import { statusText } from "./StatusBadge";

interface Handlers<T> {
  selected: boolean;
  onSelect: (path: string, toggle: boolean, range?: boolean) => void;
  onOpen: (item: T) => void;
  onContextMenu: (item: T, x: number, y: number) => void;
}

function isCut(el: HTMLElement): boolean {
  return el.scrollWidth > el.clientWidth + 1 || el.scrollHeight > el.clientHeight + 1;
}

export function pressProps<T extends { path: string }>(item: T, usable: boolean, title: string, { selected, onSelect, onOpen, onContextMenu }: Handlers<T>) {
  return {
    "data-path": item.path,
    onMouseEnter: (e: React.MouseEvent<HTMLElement>) => {
      const clips = Array.from(e.currentTarget.querySelectorAll<HTMLElement>("[data-clip]"));
      if (clips.length === 0 || clips.some(isCut)) e.currentTarget.dataset.tooltip = title;
      else delete e.currentTarget.dataset.tooltip;
    },
    onMouseLeave: (e: React.MouseEvent<HTMLElement>) => delete e.currentTarget.dataset.tooltip,
    "aria-pressed": usable ? selected : undefined,
    "aria-disabled": usable ? undefined : true,
    onClick: (e: React.MouseEvent) => usable && onSelect(item.path, e.ctrlKey || e.metaKey, e.shiftKey),
    onDoubleClick: () => usable && onOpen(item),
    onKeyDown: (e: React.KeyboardEvent) => {
      if (e.key === "Enter" && !(e.ctrlKey || e.metaKey) && usable) onOpen(item);
    },
    onContextMenu: (e: React.MouseEvent<HTMLElement>) => {
      e.preventDefault();
      e.stopPropagation();
      if (!usable) return;
      const rect = e.currentTarget.getBoundingClientRect();
      const fromKeyboard = e.clientX === 0 && e.clientY === 0;
      onContextMenu(
        item,
        fromKeyboard ? rect.left + rect.width / 2 : e.clientX,
        fromKeyboard ? rect.top + rect.height / 2 : e.clientY,
      );
    },
  };
}

export function entryProps(entry: Entry, handlers: Handlers<Entry>) {
  const status = statusText(entry);
  return pressProps(entry, entry.readable, status ? `${entry.name} (${status})` : entry.name, handlers);
}

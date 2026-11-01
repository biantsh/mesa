import type { Entry } from "../api";
import { statusText } from "./StatusBadge";

interface Handlers {
  selected: boolean;
  onSelect: (path: string, toggle: boolean) => void;
  onOpen: (entry: Entry) => void;
  onContextMenu: (entry: Entry, x: number, y: number) => void;
}

export function entryProps(entry: Entry, { selected, onSelect, onOpen, onContextMenu }: Handlers) {
  const usable = entry.readable;
  const status = statusText(entry);
  return {
    title: status ? `${entry.name} (${status})` : entry.name,
    "aria-pressed": usable ? selected : undefined,
    "aria-disabled": usable ? undefined : true,
    onClick: (e: React.MouseEvent) => usable && onSelect(entry.path, e.ctrlKey || e.metaKey),
    onDoubleClick: () => usable && onOpen(entry),
    onKeyDown: (e: React.KeyboardEvent) => {
      if (e.key === "Enter" && usable) onOpen(entry);
    },
    onContextMenu: (e: React.MouseEvent<HTMLElement>) => {
      e.preventDefault();
      e.stopPropagation();
      if (!usable) return;
      const rect = e.currentTarget.getBoundingClientRect();
      const fromKeyboard = e.clientX === 0 && e.clientY === 0;
      onContextMenu(
        entry,
        fromKeyboard ? rect.left + rect.width / 2 : e.clientX,
        fromKeyboard ? rect.top + rect.height / 2 : e.clientY,
      );
    },
  };
}

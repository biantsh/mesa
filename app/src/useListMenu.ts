import { useCallback, useState } from "react";
import type { Entry, Listed } from "./api";
import type { MenuItem } from "./components/ContextMenu";
import { counted } from "./menuTargets";
import type { ListSelection } from "./usePageSelection";
import type { ListName } from "./usePanelLayout";
import { useLatest } from "./useLatest";

interface ListMenuState {
  list: ListName;
  items: Listed[];
  x: number;
  y: number;
}

interface Inputs {
  indexed: Listed[];
  excluded: Listed[];
  listSelection: ListSelection;
  selectInList: (list: ListName, path: string, toggle: boolean) => void;
  openListed: (item: Listed) => void;
  showInFolder: (entry: Entry) => void;
  listActions: { unindex: (items: Listed[]) => void; stopExcluding: (items: Listed[]) => void };
  onOpening: () => void;
}

export function useListMenu({ indexed, excluded, listSelection, selectInList, openListed, showInFolder, listActions, onOpening }: Inputs) {
  const [menu, setMenu] = useState<ListMenuState | null>(null);
  const now = useLatest({ items: { indexed, excluded }, listSelection, onOpening });

  const open = useCallback(
    (list: ListName, item: Listed, x: number, y: number) => {
      const { items, listSelection: picked, onOpening: closeOther } = now.current;
      const inSelection = picked.list === list && picked.paths.has(item.path);
      if (!inSelection) selectInList(list, item.path, false);
      closeOther();
      setMenu({ list, items: inSelection ? items[list].filter((it) => picked.paths.has(it.path)) : [item], x, y });
    },
    [now, selectInList],
  );
  const openIndexed = useCallback((item: Listed, x: number, y: number) => open("indexed", item, x, y), [open]);
  const openExcluded = useCallback((item: Listed, x: number, y: number) => open("excluded", item, x, y), [open]);
  const close = useCallback(() => setMenu(null), []);

  const groups = (state: ListMenuState): MenuItem[][] => {
    const { list, items } = state;
    const several = items.length > 1;
    const [only] = items;
    return [
      ...(several
        ? []
        : [
            [
              { label: "Open", onSelect: () => openListed(only) },
              ...(only.kind === "drive" ? [] : [{ label: "Show in File Explorer", disabled: only.missing, onSelect: () => showInFolder(only) }]),
            ],
          ]),
      [
        list === "indexed"
          ? { label: counted("Remove from index", items.length, several), danger: true, onSelect: () => listActions.unindex(items) }
          : { label: counted("Stop excluding", items.length, several), onSelect: () => listActions.stopExcluding(items) },
      ],
    ];
  };

  return { menu, groups, openIndexed, openExcluded, close };
}

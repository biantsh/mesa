import { useCallback, useMemo, useRef, useState } from "react";
import type { Entry, Listed } from "./api";
import { releaseFocus } from "./focus";
import { howToPick, NOTHING_PICKED, pick, type Pick, type Picked } from "./selection";
import { useLatest } from "./useLatest";
import type { MarqueeHandlers } from "./useMarquee";
import type { ListName } from "./usePanelLayout";
import { useSelection } from "./useSelection";

export interface ListSelection extends Picked {
  list: ListName | null;
}

export const NO_LIST_SELECTION: ListSelection = { list: null, ...NOTHING_PICKED };

const NO_PATHS: ReadonlySet<string> = new Set();

export interface ListHandlers {
  onSelect: (path: string, toggle: boolean, range?: boolean) => void;
  onMove: (path: string, extend: boolean) => void;
  onSetSelection: (paths: ReadonlySet<string>) => void;
}

interface Inputs {
  folder: string;
  displayed: Entry[];
  enabled: boolean;
  indexed: Listed[];
  excluded: Listed[];
}

interface Sweep {
  entries: Picked;
  list: ListSelection;
  additive: boolean;
}

export function usePageSelection({ folder, displayed, enabled, indexed, excluded }: Inputs) {
  const [listSelection, setListSelection] = useState(NO_LIST_SELECTION);
  const clearListSelection = useCallback(() => setListSelection(NO_LIST_SELECTION), []);
  const {
    selected,
    active,
    anchor,
    select: selectEntry,
    move: moveEntry,
    choose: chooseEntries,
    clear: clearEntries,
  } = useSelection(folder, displayed, enabled, clearListSelection);

  const selectedNow = useLatest(selected);
  const select = useCallback(
    (path: string, toggle: boolean, range?: boolean) => {
      clearListSelection();
      selectEntry(path, toggle, range);
      if (toggle && !range && selectedNow.current.has(path)) releaseFocus();
    },
    [clearListSelection, selectEntry, selectedNow],
  );
  const move = useCallback(
    (path: string, extend: boolean) => {
      clearListSelection();
      moveEntry(path, extend);
    },
    [clearListSelection, moveEntry],
  );
  const clear = useCallback(() => {
    clearEntries();
    clearListSelection();
  }, [clearEntries, clearListSelection]);

  const order = useMemo(
    () => ({ indexed: indexed.map((item) => item.path), excluded: excluded.map((item) => item.path) }),
    [indexed, excluded],
  );
  const orderNow = useLatest(order);
  const pickInList = useCallback(
    (list: ListName, path: string, how: Pick) => {
      clearEntries();
      setListSelection((current) => ({
        list,
        ...pick(current.list === list ? current : NO_LIST_SELECTION, orderNow.current[list], path, how),
      }));
    },
    [clearEntries, orderNow],
  );
  const selectInList = useCallback(
    (list: ListName, path: string, toggle: boolean, range = false) => pickInList(list, path, howToPick(toggle, range)),
    [pickInList],
  );
  const listHandlers = useMemo(() => {
    const handlersOf = (list: ListName): ListHandlers => ({
      onSelect: (path, toggle, range) => selectInList(list, path, toggle, range),
      onMove: (path, extend) => pickInList(list, path, extend ? "range" : "replace"),
      onSetSelection: (paths) => {
        clearEntries();
        setListSelection({ list, ...NOTHING_PICKED, paths });
      },
    });
    return { indexed: handlersOf("indexed"), excluded: handlersOf("excluded") };
  }, [selectInList, pickInList, clearEntries]);
  const selectedIn = (list: ListName) => (listSelection.list === list ? listSelection.paths : NO_PATHS);
  const activeIn = (list: ListName) => (listSelection.list === list ? listSelection.active : null);

  const pickedNow = useLatest<Picked>({ paths: selected, anchor, active });
  const listPickedNow = useLatest(listSelection);
  const sweeping = useRef<Sweep | null>(null);
  const marqueeRegions = useMemo<Record<string, MarqueeHandlers>>(() => {
    const union = (before: ReadonlySet<string>, paths: string[]) => new Set([...before, ...paths]);
    const cancel = () => {
      const before = sweeping.current;
      if (!before) return;
      chooseEntries(before.entries);
      setListSelection(before.list);
    };
    const begin = (additive: boolean) => {
      sweeping.current = { entries: pickedNow.current, list: listPickedNow.current, additive };
    };
    const files: MarqueeHandlers = {
      begin: (additive) => {
        begin(additive);
        clearListSelection();
      },
      sweep: (paths) => {
        const before = sweeping.current;
        if (!before) return;
        chooseEntries({
          paths: before.additive ? union(before.entries.paths, paths) : new Set(paths),
          anchor: paths[0] ?? before.entries.anchor,
          active: paths.length > 0 ? paths[paths.length - 1] : before.entries.active,
        });
      },
      cancel,
    };
    const listed = (list: ListName): MarqueeHandlers => ({
      begin: (additive) => {
        begin(additive);
        clearEntries();
      },
      sweep: (paths) => {
        const before = sweeping.current;
        if (!before) return;
        const own = before.list.list === list ? before.list : NO_LIST_SELECTION;
        setListSelection({
          list,
          paths: before.additive ? union(own.paths, paths) : new Set(paths),
          anchor: paths[0] ?? own.anchor,
          active: paths.length > 0 ? paths[paths.length - 1] : own.active,
        });
      },
      cancel,
    });
    return { files, indexed: listed("indexed"), excluded: listed("excluded") };
  }, [chooseEntries, clearEntries, clearListSelection, pickedNow, listPickedNow]);

  return {
    selected,
    active,
    select,
    move,
    clear,
    listSelection,
    clearListSelection,
    selectInList,
    listHandlers,
    selectedIn,
    activeIn,
    marqueeRegions,
  };
}

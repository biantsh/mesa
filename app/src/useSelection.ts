import { useCallback, useEffect, useMemo, useState } from "react";
import type { Entry } from "./api";
import { shapeOf } from "./entriesShape";
import { everything, howToPick, NOTHING_PICKED, pick, type Pick, type Picked } from "./selection";
import { useLatest } from "./useLatest";

export function useSelection(folder: string, entries: Entry[], enabled = true, onSelectAll?: () => void) {
  const [selection, setSelection] = useState({ folder, ...NOTHING_PICKED });
  if (selection.folder !== folder) setSelection({ folder, ...NOTHING_PICKED });
  const current: Picked = selection.folder === folder ? selection : NOTHING_PICKED;
  const shape = shapeOf(entries);
  // eslint-disable-next-line react-hooks/exhaustive-deps
  const order = useMemo(() => entries.filter((entry) => entry.readable).map((entry) => entry.path), [shape]);
  const orderNow = useLatest(order);

  const pickPath = useCallback(
    (path: string, how: Pick) => {
      setSelection((state) => ({
        folder,
        ...pick(state.folder === folder ? state : NOTHING_PICKED, orderNow.current, path, how),
      }));
    },
    [folder],
  );

  const select = useCallback(
    (path: string, toggle: boolean, range = false) => pickPath(path, howToPick(toggle, range)),
    [pickPath],
  );
  const move = useCallback((path: string, extend: boolean) => pickPath(path, extend ? "range" : "replace"), [pickPath]);

  const clear = useCallback(() => setSelection({ folder, ...NOTHING_PICKED }), [folder]);

  const choose = useCallback((picked: Picked) => setSelection({ folder, ...picked }), [folder]);

  useEffect(() => {
    if (!enabled) return;
    const onKeyDown = (e: KeyboardEvent) => {
      if (!(e.ctrlKey || e.metaKey) || e.key.toLowerCase() !== "a") return;
      if (isForSomethingElse(e.target)) return;
      // No preventDefault(): WebKit would hide the pointer until it moves.
      setSelection((state) => ({
        ...(state.folder === folder ? state : { folder, ...NOTHING_PICKED }),
        folder,
        paths: e.shiftKey ? NOTHING_PICKED.paths : everything(entries),
      }));
      if (!e.shiftKey) onSelectAll?.();
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [folder, entries, enabled, onSelectAll]);

  return { selected: current.paths, active: current.active, anchor: current.anchor, select, move, choose, clear };
}

export function isForSomethingElse(target: EventTarget | null): boolean {
  return (
    target instanceof HTMLElement &&
    (target.isContentEditable ||
      target.closest("input, textarea, select, dialog, [role='menu'], [data-own-keys]") !== null)
  );
}

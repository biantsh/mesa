import { useEffect, type RefObject } from "react";
import { releaseFocus } from "./focus";
import type { IndexActions } from "./useIndexActions";
import { useLatest } from "./useLatest";
import { isForSomethingElse } from "./useSelection";

interface Options {
  enabled: boolean;
  somethingSelected: boolean;
  dragging: RefObject<boolean>;
  clear: () => void;
  actions: IndexActions;
}

export function useIndexKeys({ enabled, somethingSelected, dragging, clear, actions }: Options) {
  const selectedNow = useLatest(somethingSelected);
  useEffect(() => {
    if (!enabled) return;
    const onKeyDown = (e: KeyboardEvent) => {
      if (e.key !== "Escape" || e.defaultPrevented || !selectedNow.current || dragging.current) return;
      const target = e.target;
      if (
        target instanceof HTMLElement &&
        (target.isContentEditable || target.closest("input, textarea, select, dialog, [role='menu']") !== null)
      ) {
        return;
      }
      e.preventDefault();
      clear();
      releaseFocus();
    };
    window.addEventListener("keydown", onKeyDown, true);
    return () => window.removeEventListener("keydown", onKeyDown, true);
  }, [enabled, clear, dragging, selectedNow]);

  const actionsNow = useLatest(actions);
  useEffect(() => {
    if (!enabled) return;
    const onKeyDown = (e: KeyboardEvent) => {
      if (e.defaultPrevented || e.altKey || e.shiftKey || isForSomethingElse(e.target)) return;
      const keys = actionsNow.current;
      if (e.key === "Enter" && (e.ctrlKey || e.metaKey) && keys.add > 0) {
        e.preventDefault();
        keys.doAdd();
      } else if (e.key === "Delete" && !e.ctrlKey && !e.metaKey && (keys.takeOut > 0 || keys.drop > 0)) {
        e.preventDefault();
        if (keys.takeOut > 0) keys.doTakeOut();
        else keys.doDrop();
      }
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [enabled, actionsNow]);
}

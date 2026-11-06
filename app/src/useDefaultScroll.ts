import { useEffect } from "react";
import { scrollByKey } from "./scrollKeys";
import { useLatest } from "./useLatest";
import { isForSomethingElse } from "./useSelection";

export function useDefaultScroll(enabled: boolean, scroller: () => HTMLElement | null) {
  const latest = useLatest(scroller);

  useEffect(() => {
    if (!enabled) return;
    const onKeyDown = (e: KeyboardEvent) => {
      if (e.defaultPrevented || e.ctrlKey || e.altKey || e.metaKey || e.shiftKey) return;
      if (isForSomethingElse(e.target)) return;
      if (e.target instanceof Element && e.target.closest("[data-scroller]")) return;
      const view = latest.current();
      if (view && scrollByKey(view, e.key, e.repeat)) e.preventDefault();
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [enabled, latest]);
}

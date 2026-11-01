import { useCallback, useEffect, useState } from "react";
import type { Entry } from "./api";
import { clicked, everything } from "./selection";

const NOTHING: ReadonlySet<string> = new Set();

export function useSelection(folder: string, entries: Entry[]) {
  const [selection, setSelection] = useState({ folder, paths: NOTHING });
  if (selection.folder !== folder) setSelection({ folder, paths: NOTHING });
  const selected = selection.folder === folder ? selection.paths : NOTHING;

  const select = useCallback(
    (path: string, toggle: boolean) => {
      setSelection((current) => ({
        folder,
        paths: clicked(current.folder === folder ? current.paths : NOTHING, path, toggle),
      }));
    },
    [folder],
  );

  const clear = useCallback(() => setSelection({ folder, paths: NOTHING }), [folder]);

  useEffect(() => {
    const onKeyDown = (e: KeyboardEvent) => {
      if (!(e.ctrlKey || e.metaKey) || e.key.toLowerCase() !== "a") return;
      if (isForSomethingElse(e.target)) return;
      e.preventDefault();
      setSelection({ folder, paths: e.shiftKey ? NOTHING : everything(entries) });
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [folder, entries]);

  return { selected, select, clear };
}

function isForSomethingElse(target: EventTarget | null): boolean {
  return (
    target instanceof HTMLElement &&
    (target.isContentEditable || target.closest("input, textarea, select, dialog, [role='menu']") !== null)
  );
}

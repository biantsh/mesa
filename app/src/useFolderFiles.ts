import { useCallback, useEffect, useMemo, useState } from "react";
import { api, type Entry } from "./api";

export interface FolderFiles {
  total: number;
  counts: ReadonlyMap<string, number>;
}

const KEY = "mesa.folderFill";
const RISE_MS = 300;

function stored(): boolean {
  try {
    return localStorage.getItem(KEY) !== "off";
  } catch {
    return true;
  }
}

export function useFolderFill(): [boolean, (on: boolean) => void, boolean] {
  const [on, setOn] = useState(stored);
  const [rising, setRising] = useState(false);
  useEffect(() => {
    if (!rising) return;
    const timer = setTimeout(() => setRising(false), RISE_MS);
    return () => clearTimeout(timer);
  }, [rising]);
  const change = useCallback((next: boolean) => {
    setOn(next);
    setRising(next);
    try {
      localStorage.setItem(KEY, next ? "on" : "off");
    } catch {
    }
  }, []);
  return [on, change, rising];
}

export function useFolderFiles(folder: string | null, entries: Entry[], types: string): FolderFiles | null {
  const key = useMemo(
    () => JSON.stringify(entries.filter((entry) => entry.kind === "folder").map((entry) => [entry.path, entry.index === "excluded"])),
    [entries],
  );
  const paths = useMemo(() => (JSON.parse(key) as [string, boolean][]).map(([path]) => path), [key]);
  const [found, setFound] = useState<{ folder: string; files: FolderFiles } | null>(null);

  useEffect(() => {
    if (!folder || paths.length === 0) return;
    let gone = false;
    api
      .folderFileCounts(folder, paths)
      .then((said) => {
        if (gone || !said) return;
        setFound({ folder, files: { total: said.total, counts: new Map(paths.map((path, i) => [path, said.counts[i]])) } });
      })
      .catch(() => {});
    return () => {
      gone = true;
    };
  }, [folder, paths, types]);

  return found?.folder === folder ? found.files : null;
}

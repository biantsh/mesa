import { useEffect, useRef } from "react";
import { api } from "./api";
import { FOLDER_CHANGED } from "./events";
import { useTauriEvent } from "./useTauriEvent";

export function useFolderChanges(folder: string | null, active: boolean, refresh: () => void) {
  const watching = active ? folder : null;
  useEffect(() => {
    api.watchShownFolder(watching).catch(() => {});
  }, [watching]);
  useEffect(() => () => void api.watchShownFolder(null).catch(() => {}), []);

  const wasActive = useRef(active);
  useEffect(() => {
    if (active && !wasActive.current && folder !== null) refresh();
    wasActive.current = active;
  }, [active, folder, refresh]);

  useTauriEvent<string>(FOLDER_CHANGED, (changed) => {
    if (active && changed === folder) refresh();
  });
}

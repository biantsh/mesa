import type { SearchHit } from "./api";
import type { MenuItem } from "./components/ContextMenu";
import { fileManager } from "./platform";

export interface ResultActions {
  open: (hit: SearchHit) => void;
  openAt: (hit: SearchHit, line: number) => void;
  showInFolder: (hit: SearchHit) => void;
  copyFiles: (hits: SearchHit[]) => Promise<void>;
  copyPaths: (hits: SearchHit[]) => Promise<void>;
}

export function resultMenuGroups(hits: SearchHit[], actions: ResultActions): MenuItem[][] {
  const several = hits.length > 1;
  const [only] = hits;
  return [
    ...(several
      ? []
      : [
          [
            { label: "Open", onSelect: () => actions.open(only) },
            { label: `Show in ${fileManager}`, onSelect: () => actions.showInFolder(only) },
          ],
        ]),
    [
      { label: several ? "Copy files" : "Copy file", onSelect: () => void actions.copyFiles(hits).catch(() => {}) },
      { label: several ? "Copy paths" : "Copy path", onSelect: () => void actions.copyPaths(hits).catch(() => {}) },
    ],
  ];
}

import type { Entry } from "./api";
import type { MenuItem } from "./components/ContextMenu";
import { counted } from "./menuTargets";
import type { IndexActions } from "./useIndexActions";
import { fileManager } from "./platform";

export function menuGroups(
  targets: Entry[],
  actions: IndexActions,
  open: (entry: Entry) => void,
  showInFolder: (entry: Entry) => void,
): MenuItem[][] {
  const several = targets.length > 1;
  const [only] = targets;
  return [
    ...(several
      ? []
      : [
          [
            { label: "Open", onSelect: () => open(only) },
            ...(only.kind === "drive" ? [] : [{ label: `Show in ${fileManager}`, onSelect: () => showInFolder(only) }]),
          ],
        ]),
    [
      {
        label: counted("Add to index", actions.add, several),
        disabled: actions.add === 0,
        onSelect: actions.doAdd,
      },
    ],
    [
      actions.exclude === 0 && actions.stop > 0
        ? { label: "Stop excluding", onSelect: actions.doStopExcluding }
        : { label: actions.excludeLabel, disabled: actions.exclude === 0, onSelect: actions.doExclude },
      ...(actions.remove > 0
        ? [{ label: counted("Remove from index", actions.remove, several), danger: true, onSelect: actions.doRemove }]
        : []),
    ],
  ];
}

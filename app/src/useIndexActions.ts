import { useCallback, useState } from "react";
import { api, errorText, type Entry } from "./api";
import { CONFIRMATIONS, type Confirmation, type ConfirmContext } from "./confirmations";
import { describeKinds, excludeLabel, menuTargets, wholeFolderAdd } from "./menuTargets";

interface Inputs {
  folder: string | null;
  entries: Entry[];
  shownFolder: { readonly current: string | null };
  load: (path: string) => Promise<void>;
  refreshProgress: () => Promise<void>;
  clearSelection: () => void;
  setError: (message: string | null) => void;
  markStopped: () => void;
}

type Asking = ({ which: Confirmation; subject: string } & Partial<ConfirmContext>) | null;

export type IndexActions = ReturnType<ReturnType<typeof useIndexActions>["actionsFor"]>;

export function useIndexActions({
  folder,
  entries,
  shownFolder,
  load,
  refreshProgress,
  clearSelection,
  setError,
  markStopped,
}: Inputs) {
  const [asking, setAsking] = useState<Asking>(null);

  const changeIndex = useCallback(
    async (action: () => Promise<void>, afterwards?: () => void) => {
      try {
        await action();
        await refreshProgress();
        afterwards?.();
        const shown = shownFolder.current;
        if (shown !== null) await load(shown);
      } catch (e) {
        setError(`Couldn't update the index. ${errorText(e)}`);
      }
    },
    [load, refreshProgress, setError, shownFolder],
  );

  const changeSelection = (action: () => Promise<void>) => changeIndex(action, clearSelection);

  const actionsFor = (targets: Entry[]) => {
    const { add, drop, remove, exclude, stop } = menuTargets(targets);
    const paths = (of: Entry[]) => of.map((entry) => entry.path);
    return {
      add: add.length,
      drop: drop.length,
      remove: remove.length,
      exclude: exclude.length,
      excludeLabel: excludeLabel(targets, exclude),
      stop: stop.length,
      doAdd: () => {
        const whole = folder === null ? null : wholeFolderAdd(folder, entries, targets, add);
        return changeSelection(() =>
          whole === "folder" && folder !== null
            ? api.addToIndex([folder])
            : api.addToIndex(paths(add), whole === "completing" && folder !== null ? folder : undefined),
        );
      },
      doDrop: () => {
        markStopped();
        return changeSelection(() => api.dropFromQueue(paths(drop)));
      },
      doRemove: () =>
        remove.length > 1
          ? setAsking({ which: "removeFromIndex", subject: `${remove.length} items`, paths: paths(remove) })
          : changeSelection(() => api.removeFromIndex(paths(remove))),
      doExclude: async () => {
        try {
          const withData = await api.countWithData(paths(exclude));
          const stops = exclude.some((entry) => entry.droppable);
          if (withData > 1) {
            setAsking({ which: "excludeFromIndex", subject: describeKinds(exclude), paths: paths(exclude), withData, stops });
          } else {
            if (stops) markStopped();
            await changeSelection(() => api.excludeFromIndex(paths(exclude)));
          }
        } catch (e) {
          setError(`Couldn't update the index. ${errorText(e)}`);
        }
      },
      doStopExcluding: () => changeSelection(() => api.stopExcluding(paths(stop))),
    };
  };

  const askAboutExclusions = async (which: "indexAll" | "clearIndex") => {
    try {
      setAsking({ which, subject: "", exclusions: await api.exclusionCount() });
    } catch (e) {
      setError(errorText(e));
    }
  };

  const askToCancel = (subject: string) => setAsking({ which: "cancelIndexing", subject });

  const confirmation = asking
    ? CONFIRMATIONS[asking.which](asking.subject, {
        paths: asking.paths ?? [],
        exclusions: asking.exclusions ?? 0,
        withData: asking.withData ?? 0,
        stops: asking.stops ?? false,
      })
    : null;

  const confirm = () => {
    if (!asking || !confirmation) return;
    setAsking(null);
    if (confirmation.stopsIndexing) markStopped();
    const ofSelection = asking.which === "removeFromIndex" || asking.which === "excludeFromIndex";
    changeIndex(confirmation.run, ofSelection ? clearSelection : undefined);
  };

  return { actionsFor, askAboutExclusions, askToCancel, confirmation, confirm, dismiss: () => setAsking(null) };
}

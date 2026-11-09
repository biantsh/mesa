import { useCallback, useMemo, useRef, useState } from "react";
import { api, errorText, type Entry, type Listed } from "./api";
import { CONFIRMATIONS, removeItems, type Confirmation, type ConfirmContext } from "./confirmations";
import { describeKinds, excludeLabel, menuTargets, wholeFolderAdd } from "./menuTargets";

interface Inputs {
  folder: string | null;
  entries: Entry[];
  shownFolder: { readonly current: string | null };
  load: (path: string) => Promise<void>;
  refreshProgress: () => Promise<void>;
  refreshLists: () => Promise<void>;
  clearSelection: () => void;
  clearListSelection: () => void;
  setError: (message: string | null) => void;
  markStopped: () => void;
}

const UNDO_ADD_MS = 800;

type Asking = ({ which: Confirmation; subject: string } & Partial<ConfirmContext>) | null;

export type IndexActions = ReturnType<ReturnType<typeof useIndexActions>["actionsFor"]>;

export function useIndexActions({
  folder,
  entries,
  shownFolder,
  load,
  refreshProgress,
  refreshLists,
  clearSelection,
  clearListSelection,
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
        await Promise.all([shown !== null ? load(shown) : undefined, refreshLists()]);
      } catch (e) {
        setError(`Couldn't update the index. ${errorText(e)}`);
      }
    },
    [load, refreshProgress, refreshLists, setError, shownFolder],
  );

  const changeSelection = (action: () => Promise<void>) => changeIndex(action, clearSelection);

  const actionsFor = (targets: Entry[]) => {
    const { add, drop, remove, inside, exclude, stop } = menuTargets(targets);
    const paths = (of: Entry[]) => of.map((entry) => entry.path);
    const doRemove = () =>
      remove.length > 1
        ? setAsking({ which: "removeFromIndex", subject: `${remove.length} items`, paths: paths(remove) })
        : changeSelection(() => api.removeFromIndex(paths(remove)));
    const excludeEntries = async (which: Entry[]) => {
      try {
        const withData = await api.countWithData(paths(which));
        const stops = which.some((entry) => entry.droppable);
        if (withData > 1) {
          setAsking({ which: "excludeFromIndex", subject: describeKinds(which), paths: paths(which), withData, stops });
        } else {
          if (stops) markStopped();
          await changeSelection(() => api.excludeFromIndex(paths(which)));
        }
      } catch (e) {
        setError(`Couldn't update the index. ${errorText(e)}`);
      }
    };
    return {
      add: add.length,
      drop: drop.length,
      remove: remove.length,
      takeOut: remove.length + inside.length,
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
      doRemove,
      doExclude: () => excludeEntries(exclude),
      doTakeOut: () => (remove.length > 0 ? doRemove() : excludeEntries(inside)),
      doStopExcluding: () => changeSelection(() => api.stopExcluding(paths(stop))),
    };
  };

  const listActions = useMemo(() => ({
    unindex: (items: Listed[]) => {
      const paths = items.map((item) => item.path);
      const dropping = items.filter((item) => item.droppable).map((item) => item.path);
      if (items.length > 1) {
        setAsking({ which: "removeFromIndex", subject: describeKinds(items), paths, dropping });
        return;
      }
      if (dropping.length > 0) markStopped();
      void changeIndex(() => removeItems(paths, dropping), clearListSelection);
    },
    stopExcluding: (items: Listed[]) =>
      void changeIndex(() => api.stopExcluding(items.map((item) => item.path)), clearListSelection),
  }), [changeIndex, markStopped, clearListSelection]);

  const lastAdded = useRef<{ path: string; at: number; done: Promise<void> } | null>(null);
  const addEntry = useCallback(
    (entry: Entry) => {
      lastAdded.current = {
        path: entry.path,
        at: Date.now(),
        done: changeIndex(() => api.addToIndex([entry.path])),
      };
    },
    [changeIndex],
  );

  const lastStopped = useRef<{ path: string; at: number; done: Promise<void> } | null>(null);
  const stopEntry = useCallback(
    (entry: Entry) => {
      markStopped();
      lastStopped.current = {
        path: entry.path,
        at: Date.now(),
        done: changeIndex(() => removeItems([entry.path], [entry.path])),
      };
    },
    [changeIndex, markStopped],
  );

  const undoRecentStop = useCallback(
    async (entry: Entry) => {
      const stopped = lastStopped.current;
      if (!stopped || stopped.path !== entry.path || Date.now() - stopped.at > UNDO_ADD_MS) return;
      lastStopped.current = null;
      try {
        await stopped.done;
        await api.addToIndex([entry.path]);
        await Promise.all([refreshProgress(), refreshLists()]);
      } catch (e) {
        setError(`Couldn't update the index. ${errorText(e)}`);
      }
    },
    [refreshProgress, refreshLists, setError],
  );

  const undoRecentAdd = useCallback(
    async (entry: Entry) => {
      const added = lastAdded.current;
      if (!added || added.path !== entry.path || Date.now() - added.at > UNDO_ADD_MS) return;
      lastAdded.current = null;
      markStopped();
      try {
        await added.done;
        await removeItems([entry.path], [entry.path]);
        await Promise.all([refreshProgress(), refreshLists()]);
      } catch (e) {
        setError(`Couldn't update the index. ${errorText(e)}`);
      }
    },
    [markStopped, refreshProgress, refreshLists, setError],
  );

  const askAboutIndexAll = async () => {
    try {
      setAsking({ which: "indexAll", subject: "", exclusions: await api.exclusionCount() });
    } catch (e) {
      setError(errorText(e));
    }
  };

  const confirmation = asking
    ? CONFIRMATIONS[asking.which](asking.subject, {
        paths: asking.paths ?? [],
        exclusions: asking.exclusions ?? 0,
        withData: asking.withData ?? 0,
        stops: asking.stops ?? false,
        dropping: asking.dropping ?? [],
      })
    : null;

  const confirm = () => {
    if (!asking || !confirmation) return;
    setAsking(null);
    if (confirmation.stopsIndexing) markStopped();
    const ofSelection = asking.which === "removeFromIndex" || asking.which === "excludeFromIndex";
    changeIndex(confirmation.run, ofSelection ? clearSelection : undefined);
  };

  return {
    actionsFor,
    addEntry,
    stopEntry,
    undoRecentAdd,
    undoRecentStop,
    listActions,
    askAboutIndexAll,
    confirmation,
    confirm,
    dismiss: () => setAsking(null),
  };
}

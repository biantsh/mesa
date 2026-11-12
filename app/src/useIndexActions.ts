import { useCallback, useMemo, useRef, useState } from "react";
import { api, errorText, type Entry, type Listed } from "./api";
import { CONFIRMATIONS, type Confirmation, type ConfirmContext } from "./confirmations";
import { describeKinds, excludeLabel, menuTargets, whenAdding } from "./menuTargets";

interface Inputs {
  folder: string | null;
  entries: Entry[];
  refresh: () => Promise<void>;
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
  refresh,
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
      } catch (e) {
        setError(`Couldn't update the index. ${errorText(e)}`);
        return;
      }
      await refresh();
      afterwards?.();
    },
    [refresh, setError],
  );

  const changeSelection = (action: () => Promise<void>) => changeIndex(action, clearSelection);

  const actionsFor = (targets: Entry[]) => {
    const { add, remove, inside, exclude, stop } = menuTargets(targets);
    const paths = (of: Entry[]) => of.map((entry) => entry.path);
    const doRemove = () => {
      const stops = remove.some((entry) => entry.queued);
      if (remove.length > 1) {
        setAsking({ which: "removeFromIndex", subject: `${remove.length} items`, paths: paths(remove), stops });
        return Promise.resolve();
      }
      if (stops) markStopped();
      return changeSelection(() => api.removeFromIndex(paths(remove)));
    };
    const excludeEntries = async (which: Entry[]) => {
      try {
        const withData = await api.countWithData(paths(which));
        const stops = which.some((entry) => entry.queued);
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
      remove: remove.length,
      takeOut: remove.length + inside.length,
      exclude: exclude.length,
      excludeLabel: excludeLabel(targets, exclude),
      stop: stop.length,
      doAdd: () => {
        const instead = folder === null ? null : whenAdding(folder, entries, targets, add);
        if (instead && folder !== null) {
          setAsking({
            which: "indexFolderInstead",
            subject: describeKinds(add),
            paths: paths(add),
            folder,
            completing: instead.instead === "completing",
            everything: instead.everything,
          });
          return Promise.resolve();
        }
        return changeSelection(() => api.addToIndex(paths(add)));
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
      const stops = items.some((item) => item.queued);
      if (items.length > 1) {
        setAsking({ which: "removeFromIndex", subject: describeKinds(items), paths, stops });
        return;
      }
      if (stops) markStopped();
      void changeIndex(() => api.removeFromIndex(paths), clearListSelection);
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
        done: changeIndex(() => api.removeFromIndex([entry.path])),
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
        await refresh();
      } catch (e) {
        setError(`Couldn't update the index. ${errorText(e)}`);
      }
    },
    [refresh, setError],
  );

  const undoRecentAdd = useCallback(
    async (entry: Entry) => {
      const added = lastAdded.current;
      if (!added || added.path !== entry.path || Date.now() - added.at > UNDO_ADD_MS) return;
      lastAdded.current = null;
      markStopped();
      try {
        await added.done;
        await api.removeFromIndex([entry.path]);
        await refresh();
      } catch (e) {
        setError(`Couldn't update the index. ${errorText(e)}`);
      }
    },
    [markStopped, refresh, setError],
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
        folder: asking.folder ?? "",
        completing: asking.completing ?? false,
        everything: asking.everything ?? false,
      })
    : null;

  const confirm = () => {
    if (!asking || !confirmation) return;
    setAsking(null);
    if (confirmation.stopsIndexing) markStopped();
    changeIndex(confirmation.run, ofSelection(asking.which) ? clearSelection : undefined);
  };

  const chooseAlternative = () => {
    const alternative = confirmation?.alternative;
    if (!asking || !alternative) return;
    setAsking(null);
    changeIndex(alternative.run, ofSelection(asking.which) ? clearSelection : undefined);
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
    chooseAlternative,
    dismiss: () => setAsking(null),
  };
}

const ofSelection = (which: Confirmation) => which === "removeFromIndex" || which === "excludeFromIndex" || which === "indexFolderInstead";

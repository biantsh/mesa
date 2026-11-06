import { useMemo, useState, type Dispatch, type SetStateAction } from "react";
import { api, type Entry, type IndexState, type Listing } from "./api";
import { usePoll } from "./usePoll";

const isUnfinished = (entry: Entry) => entry.index === "indexing" || entry.index === "paused" || entry.droppable;

const mayStart = (entry: Entry) =>
  (entry.kind === "folder" || entry.kind === "drive") && (entry.index === "indexed" || entry.index === "partlyIndexed");

const mayStartIn = (entry: Entry) => entry.kind === "file" && entry.index === "indexed";

const inProgress = (state: IndexState | undefined) => state?.status === "indexing" || state?.status === "paused";

const sameState = (entry: Entry, state: IndexState) =>
  entry.index === state.status && entry.droppable === state.droppable && entry.excludedHere === state.excludedHere;

export function useLiveIndexStatus(
  listing: Listing | null,
  setListing: Dispatch<SetStateAction<Listing | null>>,
  refreshProgress: () => Promise<void>,
  busy: boolean,
) {
  const [folderBusy, setFolderBusy] = useState(false);
  const watched = useMemo(
    () =>
      listing?.entries.filter(
        (entry) => isUnfinished(entry) || (busy && mayStart(entry)) || (busy && folderBusy && mayStartIn(entry)),
      ) ?? [],
    [listing, busy, folderBusy],
  );

  const update = async () => {
    if (!listing) return;
    const all = await api.indexStatuses([...watched.map((entry) => entry.path), listing.path]);
    await refreshProgress();
    const states = all.slice(0, watched.length);
    setFolderBusy(inProgress(all[watched.length]));
    const stateOf = new Map(watched.map((entry, i) => [entry.path, states[i]]));
    setListing((current) => {
      if (current !== listing) return current;
      let changed = false;
      const entries = current.entries.map((entry) => {
        const state = stateOf.get(entry.path);
        if (!state || sameState(entry, state)) return entry;
        changed = true;
        return { ...entry, index: state.status, droppable: state.droppable, excludedHere: state.excludedHere };
      });
      return changed ? { ...current, entries } : current;
    });
  };

  usePoll(update, { active: watched.length > 0 || busy });
  return update;
}

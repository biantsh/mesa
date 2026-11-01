import { useMemo, type Dispatch, type SetStateAction } from "react";
import { api, type Entry, type IndexState, type Listing } from "./api";
import { usePoll } from "./usePoll";

const isUnfinished = (entry: Entry) => entry.index === "indexing" || entry.index === "paused";

const sameState = (entry: Entry, state: IndexState) =>
  entry.index === state.status && entry.droppable === state.droppable && entry.excludedHere === state.excludedHere;

export function useLiveIndexStatus(
  listing: Listing | null,
  setListing: Dispatch<SetStateAction<Listing | null>>,
  refreshProgress: () => Promise<void>,
) {
  const watched = useMemo(() => listing?.entries.filter(isUnfinished) ?? [], [listing]);

  usePoll(
    async () => {
      if (!listing) return;
      const states = await api.indexStatuses(watched.map((entry) => entry.path));
      await refreshProgress();
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
    },
    { active: watched.length > 0 },
  );
}

import type { Dispatch, SetStateAction } from "react";
import { api, type Listing } from "./api";
import { usePoll } from "./usePoll";

export function useLiveIndexStatus(
  listing: Listing | null,
  setListing: Dispatch<SetStateAction<Listing | null>>,
  refreshProgress: () => Promise<void>,
) {
  const unfinished =
    listing?.entries.some((e) => ["indexing", "paused"].includes(e.index)) ?? false;

  usePoll(
    async () => {
      if (!listing) return;
      const states = await api.indexStatuses(listing.entries.map((e) => e.path));
      await refreshProgress();
      setListing((current) => {
        if (current !== listing) return current;
        const unchanged = current.entries.every(
          (e, i) => e.index === states[i].status && e.droppable === states[i].droppable,
        );
        if (unchanged) return current;
        return {
          ...current,
          entries: current.entries.map((e, i) => ({
            ...e,
            index: states[i].status,
            droppable: states[i].droppable,
          })),
        };
      });
    },
    { active: unfinished },
  );
}

import type { Dispatch, SetStateAction } from "react";
import { api, type Listing } from "./api";
import { sameShape } from "./entriesShape";
import { withStatusChanges } from "./listingColumns";
import { usePoll } from "./usePoll";

export function useLiveIndexStatus(
  listing: Listing | null,
  setListing: Dispatch<SetStateAction<Listing | null>>,
  refreshProgress: () => Promise<void>,
  busy: boolean,
  onStale: () => void,
) {
  const update = async () => {
    if (!listing) return;
    const told = await api.folderStatuses(listing.generation);
    await refreshProgress();
    if (told.stale) return onStale();
    setListing((current) => {
      if (!current || current.generation !== told.base) return current;
      const entries = withStatusChanges(current.entries, told.changes);
      if (!entries) {
        return current.generation === told.generation && current.unfinished === told.unfinished
          ? current
          : { ...current, generation: told.generation, unfinished: told.unfinished };
      }
      return { ...current, entries: sameShape(entries, current.entries), generation: told.generation, unfinished: told.unfinished };
    });
  };

  usePoll(update, { active: listing !== null && (listing.unfinished || busy) });
  return update;
}

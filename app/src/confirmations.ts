import { api } from "./api";

export type Confirmation = "indexAll" | "clearIndex" | "cancelIndexing" | "removeFromIndex" | "excludeFromIndex";

export interface ConfirmationText {
  title: string;
  confirmLabel: string;
  danger: boolean;
  stopsIndexing: boolean;
  message: (string | { note: string })[];
  run: () => Promise<void>;
}

export interface ConfirmContext {
  paths: string[];
  exclusions: number;
  withData: number;
  stops: boolean;
}

export const CONFIRMATIONS: Record<Confirmation, (subject: string, context: ConfirmContext) => ConfirmationText> = {
  indexAll: (_, { exclusions }) => ({
    title: "Index this entire computer?",
    confirmLabel: "Index everything",
    danger: false,
    stopsIndexing: false,
    message: [
      "Mesa will index every supported file on this computer, across all of its drives.",
      "This can take a long time and use a lot of disk and processing power.",
      ...(exclusions > 0 ? [{ note: "Your excluded items will not be indexed." }] : []),
    ],
    run: api.indexEverything,
  }),
  clearIndex: (_, { exclusions }) => ({
    title: "Clear the entire index?",
    confirmLabel: "Clear index",
    danger: true,
    stopsIndexing: true,
    message: [
      "This removes every file and folder from Mesa's index. Your files themselves are not touched.",
      "Anything you want to search for will need to be indexed again.",
      ...(exclusions > 0 ? [{ note: "Your excluded items will no longer be excluded." }] : []),
    ],
    run: api.clearIndex,
  }),
  cancelIndexing: (subject) => ({
    title: `Cancel indexing ${subject}?`,
    confirmLabel: "Stop and remove",
    danger: true,
    stopsIndexing: true,
    message: [
      `Mesa will stop and remove ${subject} from the index, including everything already indexed from ${subject.includes(" and ") ? "them" : "it"}.`,
    ],
    run: api.cancelIndexing,
  }),
  removeFromIndex: (subject, { paths }) => ({
    title: `Remove ${subject} from the index?`,
    confirmLabel: "Remove",
    danger: true,
    stopsIndexing: false,
    message: [
      `Mesa will remove ${subject} from the index. Your files themselves are not touched.`,
    ],
    run: () => api.removeFromIndex(paths),
  }),
  excludeFromIndex: (subject, { paths, withData, stops }) => ({
    title: `Exclude ${subject}?`,
    confirmLabel: "Exclude",
    danger: true,
    stopsIndexing: stops,
    message: [
      `Mesa will leave ${subject} out of the index from now on. ` +
        (withData >= paths.length
          ? "What was already indexed of them will be removed."
          : `What was already indexed of ${withData} of them will be removed.`),
    ],
    run: () => api.excludeFromIndex(paths),
  }),
};

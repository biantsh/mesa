import { api } from "./api";

export type Confirmation = "indexAll" | "clearIndex" | "cancelIndexing";

export interface ConfirmationText {
  title: string;
  confirmLabel: string;
  danger: boolean;
  stopsIndexing: boolean;
  message: string[];
  run: () => Promise<void>;
}

export const CONFIRMATIONS: Record<Confirmation, (subject: string) => ConfirmationText> = {
  indexAll: () => ({
    title: "Index this entire computer?",
    confirmLabel: "Index everything",
    danger: false,
    stopsIndexing: false,
    message: [
      "Mesa will index every supported file on this computer, across all of its drives.",
      "This can take a long time and use a lot of disk and processing power.",
    ],
    run: api.indexEverything,
  }),
  clearIndex: () => ({
    title: "Clear the entire index?",
    confirmLabel: "Clear index",
    danger: true,
    stopsIndexing: true,
    message: [
      "This removes every file and folder from Mesa's index. Your files themselves are not touched.",
      "Anything you want to search for will need to be indexed again.",
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
};

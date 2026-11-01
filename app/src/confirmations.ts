import { api } from "./api";

export type Confirmation = "indexAll" | "clearIndex";

interface ConfirmationText {
  title: string;
  confirmLabel: string;
  danger: boolean;
  message: string[];
  run: () => Promise<void>;
}

export const CONFIRMATIONS: Record<Confirmation, ConfirmationText> = {
  indexAll: {
    title: "Index this entire computer?",
    confirmLabel: "Index everything",
    danger: false,
    message: [
      "Mesa will index every supported file on this computer, across all of its drives.",
      "This can take a long time and use a lot of disk and processing power.",
    ],
    run: api.indexEverything,
  },
  clearIndex: {
    title: "Clear the entire index?",
    confirmLabel: "Clear index",
    danger: true,
    message: [
      "This removes every file and folder from Mesa's index. Your files themselves are not touched.",
      "Anything you want to search for will need to be indexed again.",
    ],
    run: api.clearIndex,
  },
};

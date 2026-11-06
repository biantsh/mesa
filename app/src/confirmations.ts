import { api } from "./api";

export type Confirmation = "indexAll" | "removeFromIndex" | "excludeFromIndex";

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
  dropping: string[];
}

export async function removeItems(paths: string[], dropping: string[]): Promise<void> {
  if (dropping.length > 0) await api.dropFromQueue(dropping).catch(() => undefined);
  await api.removeFromIndex(paths);
}

export const CONFIRMATIONS: Record<Confirmation, (subject: string, context: ConfirmContext) => ConfirmationText> = {
  indexAll: (_, { exclusions }) => ({
    title: "Index all your files?",
    confirmLabel: "Index my files",
    danger: false,
    stopsIndexing: false,
    message: [
      "This will index every supported file in your user folder, which can take a long time.",
      "Indexing runs gently in the background unless Turbo indexing is on.",
      ...(exclusions > 0 ? [{ note: "Your excluded items will not be indexed." }] : []),
    ],
    run: api.indexEverything,
  }),
  removeFromIndex: (subject, { paths, dropping }) => ({
    title: `Remove ${subject} from the index?`,
    confirmLabel: "Remove",
    danger: true,
    stopsIndexing: dropping.length > 0,
    message: [
      `This will remove ${subject} from the index. Your files themselves are not touched.`,
    ],
    run: () => removeItems(paths, dropping),
  }),
  excludeFromIndex: (subject, { paths, withData, stops }) => ({
    title: `Exclude ${subject}?`,
    confirmLabel: "Exclude",
    danger: true,
    stopsIndexing: stops,
    message: [
      `This will leave ${subject} out of the index from now on. ` +
        (withData >= paths.length
          ? "What was already indexed of them will be removed."
          : `What was already indexed of ${withData} of them will be removed.`),
    ],
    run: () => api.excludeFromIndex(paths),
  }),
};

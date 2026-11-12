import { api } from "./api";
import { nameOf } from "./paths";

export type Confirmation = "indexAll" | "removeFromIndex" | "excludeFromIndex" | "indexFolderInstead";

export interface ConfirmationText {
  title: string;
  confirmLabel: string;
  danger: boolean;
  stopsIndexing: boolean;
  message: (string | { note: string })[];
  run: () => Promise<void>;
  alternative?: { label: string; run: () => Promise<void> };
  noCancel?: boolean;
}

export interface ConfirmContext {
  paths: string[];
  exclusions: number;
  withData: number;
  stops: boolean;
  folder: string;
  completing: boolean;
  everything: boolean;
}

export const CONFIRMATIONS: Record<Confirmation, (subject: string, context: ConfirmContext) => ConfirmationText> = {
  indexAll: (_, { exclusions }) => ({
    title: "Index all your files?",
    confirmLabel: "Index my files",
    danger: false,
    stopsIndexing: false,
    message: [
      "This will index every supported file in your user folder, which can take a long time.",
      "Indexing runs in the background, and keeps going if you close the window.",
      ...(exclusions > 0 ? [{ note: "Your excluded items will not be indexed." }] : []),
    ],
    run: api.indexEverything,
  }),
  removeFromIndex: (subject, { paths, stops }) => ({
    title: `Remove ${subject} from the index?`,
    confirmLabel: "Remove",
    danger: true,
    stopsIndexing: stops,
    message: [
      `This will remove ${subject} from the index. Your files themselves are not touched.`,
    ],
    run: () => api.removeFromIndex(paths),
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
  indexFolderInstead: (_, { paths, folder, completing, everything: all }) => {
    const name = nameOf(folder);
    return {
      title: `Index “${name}” instead?`,
      confirmLabel: "Index the folder",
      danger: false,
      stopsIndexing: false,
      noCancel: true,
      message: [
        all
          ? `You selected everything in “${name}”. Indexing the folder covers all of it, and also whatever is put in it later.`
          : `You selected ${paths.length.toLocaleString()} items in “${name}”. Indexing the folder covers them and everything else in it, and also whatever is put in it later.`,
      ],
      run: () => (completing ? api.addToIndex(paths, folder) : api.addToIndex([folder])),
      alternative: { label: paths.length === 1 ? "Only this one" : `Only these ${paths.length.toLocaleString()}`, run: () => api.addToIndex(paths) },
    };
  },
};

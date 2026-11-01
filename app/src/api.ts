import { convertFileSrc, invoke } from "@tauri-apps/api/core";

export type Kind = "drive" | "folder" | "file";
export type Category = "image" | "video" | "audio" | "document";
export type IndexStatus = "notIndexed" | "indexing" | "paused" | "queued" | "indexed";

export interface Entry {
  name: string;
  path: string;
  kind: Kind;
  category: Category | null;
  readable: boolean;
  hasContents: boolean;
  index: IndexStatus;
  droppable: boolean;
}

export interface IndexState {
  status: IndexStatus;
  droppable: boolean;
}

export interface Crumb {
  name: string;
  path: string;
}

export interface Listing {
  path: string;
  parent: string | null;
  breadcrumbs: Crumb[];
  entries: Entry[];
}

export interface Progress {
  name: string;
  queued: number;
  counting: boolean;
  found: number;
  done: number;
  total: number;
  paused: boolean;
}

export const COMPUTER = "";

export const api = {
  homeDir: () => invoke<string>("home_dir"),
  listDir: (path: string) => invoke<Listing>("list_dir", { path }),
  indexStatuses: (paths: string[]) => invoke<IndexState[]>("index_statuses", { paths }),
  indexProgress: () => invoke<Progress | null>("index_progress"),
  setIndexingPaused: (paused: boolean) => invoke<void>("set_indexing_paused", { paused }),
  cancelIndexing: () => invoke<void>("cancel_indexing"),
  dropFromQueue: (path: string) => invoke<void>("drop_from_queue", { path }),
  addToIndex: (path: string) => invoke<void>("add_to_index", { path }),
  removeFromIndex: (path: string) => invoke<void>("remove_from_index", { path }),
  indexEverything: () => invoke<void>("index_everything"),
  clearIndex: () => invoke<void>("clear_index"),
};

export function previewUrl(path: string): string {
  return convertFileSrc(path, "thumb");
}

export function errorText(error: unknown): string {
  if (typeof error === "string") return error;
  if (error instanceof Error) return error.message;
  return String(error);
}

import { convertFileSrc, invoke } from "@tauri-apps/api/core";

export type Kind = "drive" | "folder" | "file";
export type Category = "image" | "video" | "audio" | "document";
export type IndexStatus =
  | "notIndexed"
  | "indexing"
  | "paused"
  | "indexed"
  | "partlyIndexed"
  | "unindexable"
  | "excluded";

export interface Entry {
  name: string;
  path: string;
  kind: Kind;
  category: Category | null;
  readable: boolean;
  hasContents: boolean;
  index: IndexStatus;
  droppable: boolean;
  excludedHere: boolean;
  size: number | null;
  modified: number | null;
}

export interface IndexState {
  status: IndexStatus;
  droppable: boolean;
  excludedHere: boolean;
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

export interface Overview {
  empty: boolean;
  progress: Progress | null;
  files: number;
  bytes: number;
}

export interface Settings {
  gpuAcceleration: boolean;
}

export interface SearchHit {
  path: string;
  name: string;
  folder: string;
  category: Category;
  modified: number;
  size: number;
  score: number;
}

export interface SearchPage {
  id: number;
  hits: SearchHit[];
  more: boolean;
}

export interface SearchFilter {
  category: Category | null;
  from: number | null;
  to: number | null;
}

export const COMPUTER = "";

export type ModelStatus =
  | { state: "downloading"; done: number; total: number }
  | { state: "verifying" }
  | { state: "starting" }
  | { state: "preparing" }
  | { state: "ready" }
  | { state: "failed"; reason: string; setup: boolean };

export const api = {
  homeDir: () => invoke<string>("home_dir"),
  listDir: (path: string) => invoke<Listing>("list_dir", { path }),
  indexStatuses: (paths: string[]) => invoke<IndexState[]>("index_statuses", { paths }),
  indexProgress: () => invoke<Progress | null>("index_progress"),
  indexOverview: () => invoke<Overview>("index_overview"),
  search: (query: string, filter: SearchFilter) => invoke<SearchPage>("search", { query, ...filter }),
  searchMore: (id: number) => invoke<SearchPage>("search_more", { id }),
  setIndexingPaused: (paused: boolean) => invoke<void>("set_indexing_paused", { paused }),
  cancelIndexing: () => invoke<void>("cancel_indexing"),
  dropFromQueue: (paths: string[]) => invoke<void>("drop_from_queue", { paths }),
  addToIndex: (paths: string[], completing?: string) => invoke<void>("add_to_index", { paths, completing }),
  removeFromIndex: (paths: string[]) => invoke<void>("remove_from_index", { paths }),
  openProjectPage: () => invoke<void>("open_project_page"),
  openFile: (path: string) => invoke<void>("open_file", { path }),
  copyFiles: (paths: string[]) => invoke<void>("copy_files", { paths }),
  showInFolder: (path: string) => invoke<void>("show_in_folder", { path }),
  excludeFromIndex: (paths: string[]) => invoke<void>("exclude_from_index", { paths }),
  stopExcluding: (paths: string[]) => invoke<void>("stop_excluding", { paths }),
  countWithData: (paths: string[]) => invoke<number>("count_with_data", { paths }),
  exclusionCount: () => invoke<number>("exclusion_count"),
  indexEverything: () => invoke<void>("index_everything"),
  clearIndex: () => invoke<void>("clear_index"),
  getSettings: () => invoke<Settings>("get_settings"),
  modelStatus: () => invoke<ModelStatus>("model_status"),
  retryModelSetup: () => invoke<void>("retry_model_setup"),
  embeddingGpu: () => invoke<string | null>("embedding_gpu"),
  setGpuAcceleration: (enabled: boolean) => invoke<void>("set_gpu_acceleration", { enabled }),
};

export function previewUrl(path: string): string {
  return convertFileSrc(path, "thumb");
}

export function errorText(error: unknown): string {
  if (typeof error === "string") return error;
  if (error instanceof Error) return error.message;
  return String(error);
}

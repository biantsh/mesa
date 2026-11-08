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

export interface Listed extends Entry {
  missing: boolean;
  leftOut: number;
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
  updating: boolean;
}

export function sameProgress(a: Progress | null, b: Progress | null): boolean {
  if (a === null || b === null) return a === b;
  return (
    a.name === b.name &&
    a.queued === b.queued &&
    a.counting === b.counting &&
    a.found === b.found &&
    a.done === b.done &&
    a.total === b.total &&
    a.paused === b.paused &&
    a.updating === b.updating
  );
}

export interface Overview {
  empty: boolean;
  progress: Progress | null;
  everythingIndexed: boolean;
  files: number;
  bytes: number;
}

export interface Settings {
  gpuAcceleration: boolean;
  turboIndexing: boolean;
}

export interface HotkeyParts {
  keys: string[];
  name: string;
}

export interface HotkeyStatus {
  active: HotkeyParts | null;
}

export interface HotkeyFailure {
  kind: "invalid" | "failed";
  message: string;
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
  watchShownFolder: (path: string | null) => invoke<void>("watch_shown_folder", { path }),
  indexStatuses: (paths: string[]) => invoke<IndexState[]>("index_statuses", { paths }),
  indexProgress: () => invoke<Progress | null>("index_progress"),
  indexOverview: () => invoke<Overview>("index_overview"),
  search: (query: string, filter: SearchFilter, files: string[] = []) =>
    invoke<SearchPage>("search", { query, ...filter, files }),
  searchMore: (id: number) => invoke<SearchPage>("search_more", { id }),
  setIndexingPaused: (paused: boolean) => invoke<void>("set_indexing_paused", { paused }),
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
  indexedItems: () => invoke<Listed[]>("indexed_items"),
  excludedItems: () => invoke<Listed[]>("excluded_items"),
  indexEverything: () => invoke<void>("index_everything"),
  getSettings: () => invoke<Settings>("get_settings"),
  getSearchHotkey: () => invoke<HotkeyStatus>("get_search_hotkey"),
  setSearchHotkey: (shortcut: string | null) => invoke<HotkeyStatus>("set_search_hotkey", { shortcut }),
  beginHotkeyRecording: () => invoke<void>("begin_hotkey_recording"),
  endHotkeyRecording: () => invoke<void>("end_hotkey_recording"),
  modelStatus: () => invoke<ModelStatus>("model_status"),
  retryModelSetup: () => invoke<void>("retry_model_setup"),
  embeddingGpu: () => invoke<string | null>("embedding_gpu"),
  setGpuAcceleration: (enabled: boolean) => invoke<void>("set_gpu_acceleration", { enabled }),
  setTurboIndexing: (enabled: boolean) => invoke<void>("set_turbo_indexing", { enabled }),
  pagePainted: () => invoke<void>("page_painted"),
  revealMain: () => invoke<void>("reveal_main"),
  hideWidget: () => invoke<void>("hide_widget"),
  setPicking: (picking: boolean) => invoke<void>("set_picking", { picking }),
  foldersAmong: (paths: string[]) => invoke<boolean[]>("folders_among", { paths }),
  prepareExample: (path: string) => invoke<void>("prepare_example", { path }),
  openMain: (screen: "index" | "settings") => invoke<void>("open_main", { screen }),
  setWidgetBox: (left: number, top: number, right: number, bottom: number) => invoke<void>("set_widget_box", { left, top, right, bottom }),
  showMain: () => invoke<void>("show_main"),
};

export function previewUrl(path: string): string {
  return convertFileSrc(path, "thumb");
}

export function hotkeyFailure(error: unknown): HotkeyFailure {
  if (typeof error === "object" && error !== null && "kind" in error && "message" in error) {
    const { kind, message } = error as HotkeyFailure;
    if ((kind === "invalid" || kind === "failed") && typeof message === "string") return { kind, message };
  }
  return { kind: "failed", message: errorText(error) };
}

export function errorText(error: unknown): string {
  if (typeof error === "string") return error;
  if (error instanceof Error) return error.message;
  return String(error);
}

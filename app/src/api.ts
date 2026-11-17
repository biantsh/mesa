import { convertFileSrc, invoke } from "@tauri-apps/api/core";
import { withShape, sameShape } from "./entriesShape";
import { applyRuns, entriesOf, listedOf, type Columns, type ListedColumns, type Run, type StatusChange } from "./listingColumns";

export type Kind = "drive" | "folder" | "file";
export type Category = "image" | "video" | "audio" | "document" | "code";
export type IndexStatus =
  | "notIndexed"
  | "indexing"
  | "paused"
  | "indexed"
  | "partlyIndexed"
  | "unindexable"
  | "excluded"
  | "typeNotIndexed";

export interface Entry {
  name: string;
  path: string;
  kind: Kind;
  category: Category | null;
  readable: boolean;
  hasContents: boolean;
  index: IndexStatus;
  queued: boolean;
  excludedHere: boolean;
  removable: boolean;
  size: number | null;
  modified: number | null;
  privacy?: PrivacyPane | null;
}

export type PrivacyPane = "filesAndFolders" | "photos" | "media";

export interface Notices {
  missing?: boolean;
  texts: string[];
  sections: NoticeSection[];
}

export interface NoticeSection {
  id: string;
  title: string;
  items: NoticeItem[];
}

export interface NoticeItem {
  name: string;
  version?: string;
  by?: string;
  license: string;
  note?: string;
  url?: string;
  texts: number[];
}

export interface Listed extends Entry {
  missing: boolean;
  slot: number;
}

export interface Lists {
  version: number;
  indexed: Listed[];
  excluded: Listed[];
}

interface SaidLists {
  version: number;
  indexed: ListedColumns;
  excluded: ListedColumns;
}

export interface ListStates {
  version: number;
  stale: boolean;
  states: [IndexStatus, boolean, boolean][] | null;
}

export interface IndexView {
  overview: Overview;
  folder: FolderStatuses | null;
  lists: ListStates | null;
}

export interface IndexState {
  status: IndexStatus;
  queued: boolean;
  excludedHere: boolean;
  removable: boolean;
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
  generation: number;
  shape: number;
}

interface SaidListing extends Omit<Listing, "entries"> {
  entries: Columns;
}

function listingOf(said: SaidListing): Listing {
  const entries = withShape(entriesOf(said.entries), { rust: said.shape, order: null });
  return { ...said, entries };
}

type Refreshed =
  | ({ kind: "whole" } & SaidListing)
  | {
      kind: "changes";
      base: number;
      generation: number;
      shape: number;
      runs: Run[];
      fresh: Columns;
      parent: string | null;
      breadcrumbs: Crumb[];
    };

export interface FolderStatuses {
  stale: boolean;
  base: number;
  generation: number;
  changes: StatusChange[];
  folder: IndexState | null;
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
  folders: string[];
  turbo: boolean;
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
    a.updating === b.updating &&
    a.folders.join("/") === b.folders.join("/") &&
    a.turbo === b.turbo
  );
}

export interface Overview {
  empty: boolean;
  progress: Progress | null;
  everythingIndexed: boolean;
  files: number;
  bytes: number;
}

export interface Survey {
  counts: Record<Category, number>;
  bytes: Record<Category, number>;
  notAsked: string[];
  noAccess: string[];
}

export interface Estimate {
  seconds: Record<Category, number | null>;
  measuring: boolean;
  gentleSlowdown: number;
}

export interface Settings {
  gpuAcceleration: boolean;
  alwaysFullSpeed: boolean;
  startAtSignIn: boolean;
}

export interface HotkeyParts {
  keys: string[];
  name: string;
}

export interface HotkeyStatus {
  active: HotkeyParts | null;
  chosenIn: { kind: "mesa" } | { kind: "desktop" } | { kind: "unavailable"; desktop: string | null };
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
  reason: "words" | "meaning" | "fewFiles" | "clues" | null;
  byWords: "words" | "phrase" | "name" | null;
  at: number | null;
  copies: string[];
}

export interface AlsoIn {
  path: string;
  line: number;
  symbol: string;
  similarity: number;
}

export interface MatchSnippet {
  parts: { text: string; hit: boolean }[];
  page: number | null;
}

export interface CodeView {
  language: string;
  project: string | null;
  symbol: string;
  start: number;
  end: number;
  lines: CodePart[][];
}

export type CodeKind = "keyword" | "string" | "comment" | "number" | "function" | "type" | "property" | "variable";

export interface CodePart {
  text: string;
  hit?: boolean;
  kind?: CodeKind;
}

export interface CodeFile {
  language: string;
  lines: CodePart[][];
  total: number;
}

export interface MatchDetails {
  snippets: MatchSnippet[];
  closest: MatchSnippet | null;
  code: CodeView | null;
  alsoIn?: AlsoIn[];
}

export interface SearchPage {
  id: number;
  hits: SearchHit[];
  more: boolean;
  close: boolean;
  moreClose: boolean;
}

export interface SearchFilter {
  category: Category | null;
  from: number | null;
  to: number | null;
  language?: string | null;
  project?: string | null;
  tests?: boolean | null;
}

export const COMPUTER = "";

export type ModelStatus =
  | { state: "downloading"; done: number; total: number }
  | { state: "verifying" }
  | { state: "finalizing" }
  | { state: "starting" }
  | { state: "preparing" }
  | { state: "ready" }
  | { state: "failed"; reason: string; setup: boolean };

export interface Paging {
  pages: number;
  note: string | null;
}

export const api = {
  homeDir: () => invoke<string>("home_dir"),
  listDir: async (path: string): Promise<Listing> => listingOf(await invoke<SaidListing>("list_dir", { path })),
  refreshFolder: async (shown: Listing): Promise<{ listing: Listing; whole: boolean }> => {
    const said = await invoke<Refreshed>("refresh_folder", { path: shown.path, generation: shown.generation });
    if (said.kind === "whole") return { listing: listingOf(said), whole: true };
    const entries = applyRuns(shown.entries, said.runs, entriesOf(said.fresh));
    if (!entries) return { listing: listingOf(await invoke<SaidListing>("list_dir", { path: shown.path })), whole: true };
    const { generation, shape, parent, breadcrumbs } = said;
    return { whole: false, listing: {
      ...shown,
      parent,
      breadcrumbs,
      generation,
      shape,
      entries: shape === shown.shape ? sameShape(entries, shown.entries) : withShape(entries, { rust: shape, order: null }),
    } };
  },
  watchShownFolder: (path: string | null) => invoke<void>("watch_shown_folder", { path }),
  showFolderPreviews: (generation: number, folder: FolderPreviews | null) => invoke<void>("show_folder_previews", { generation, folder }),
  showPreviewWindow: (generation: number, start: number, end: number, warm: boolean) =>
    invoke<void>("show_preview_window", { generation, start, end, warm }),
  indexView: (listing: number | null, lists: number | null, listStates: boolean) =>
    invoke<IndexView>("index_view", { listing, lists, listStates }),
  search: (query: string, filter: SearchFilter, files: string[] = []) =>
    invoke<SearchPage>("search", { query, filter, files }),
  searchMore: (id: number) => invoke<SearchPage>("search_more", { id }),
  paging: (path: string) => invoke<Paging | null>("paging", { path }),
  allowPlaying: (path: string) => invoke<void>("allow_playing", { path }),
  matchDetails: (path: string, query: string, at: number | null) => invoke<MatchDetails>("match_details", { path, query, at }),
  setIndexingPaused: (paused: boolean) => invoke<void>("set_indexing_paused", { paused }),
  setIndexingTurbo: (enabled: boolean) => invoke<void>("set_indexing_turbo", { enabled }),
  getIndexedTypes: () => invoke<Category[]>("get_indexed_types"),
  setIndexedTypes: (types: Category[]) => invoke<Category[]>("set_indexed_types", { types }),
  countIndexedOfType: (kind: Category) => invoke<number>("count_indexed_of_type", { kind }),
  addToIndex: (paths: string[], completing?: string) => invoke<void>("add_to_index", { paths, completing }),
  removeFromIndex: (paths: string[]) => invoke<void>("remove_from_index", { paths }),
  reportPageError: (message: string) => invoke<void>("report_page_error", { message }),
  openProjectPage: () => invoke<void>("open_project_page"),
  notices: () => invoke<Notices>("notices"),
  openNoticeLink: (url: string) => invoke<void>("open_notice_link", { url }),
  openPrivacySettings: (pane: PrivacyPane) => invoke<void>("open_privacy_settings", { pane }),
  openFile: (path: string) => invoke<void>("open_file", { path }),
  openCode: (path: string, line: number) => invoke<void>("open_code", { path, line }),
  codePeek: (path: string, line: number | null) => invoke<CodePart[][]>("code_peek", { path, line }),
  codeProjects: () => invoke<string[]>("code_projects"),
  alsoIn: (path: string, line: number) => invoke<AlsoIn[]>("also_in", { path, line }),
  codeFile: (path: string, query: string) => invoke<CodeFile>("code_file", { path, query }),
  copyFiles: (paths: string[]) => invoke<void>("copy_files", { paths }),
  showInFolder: (path: string) => invoke<void>("show_in_folder", { path }),
  excludeFromIndex: (paths: string[]) => invoke<void>("exclude_from_index", { paths }),
  stopExcluding: (paths: string[]) => invoke<void>("stop_excluding", { paths }),
  countWithData: (paths: string[]) => invoke<number>("count_with_data", { paths }),
  exclusionCount: () => invoke<number>("exclusion_count"),
  indexLists: async (): Promise<Lists> => {
    const said = await invoke<SaidLists>("index_lists");
    return { version: said.version, indexed: listedOf(said.indexed), excluded: listedOf(said.excluded) };
  },
  stopIndexing: () => invoke<void>("stop_indexing"),
  indexEverything: () => invoke<void>("index_everything"),
  surveyUserFiles: (includeProtected: boolean) => invoke<Survey>("survey_user_files", { includeProtected }),
  indexEstimate: () => invoke<Estimate>("index_estimate"),
  getSettings: () => invoke<Settings>("get_settings"),
  getSearchHotkey: () => invoke<HotkeyStatus>("get_search_hotkey"),
  setSearchHotkey: (shortcut: string | null) => invoke<HotkeyStatus>("set_search_hotkey", { shortcut }),
  beginHotkeyRecording: () => invoke<void>("begin_hotkey_recording"),
  endHotkeyRecording: () => invoke<void>("end_hotkey_recording"),
  modelStatus: () => invoke<ModelStatus>("model_status"),
  retryModelSetup: () => invoke<void>("retry_model_setup"),
  embeddingGpu: () => invoke<{ name: string; shared: boolean } | null>("embedding_gpu"),
  setGpuAcceleration: (enabled: boolean) => invoke<void>("set_gpu_acceleration", { enabled }),
  setAlwaysFullSpeed: (enabled: boolean) => invoke<void>("set_always_full_speed", { enabled }),
  setStartAtSignIn: (enabled: boolean) => invoke<void>("set_start_at_sign_in", { enabled }),
  pagePainted: () => invoke<void>("page_painted"),
  revealMain: () => invoke<void>("reveal_main"),
  hideWidget: () => invoke<void>("hide_widget"),
  setPicking: (picking: boolean) => invoke<void>("set_picking", { picking }),
  foldersAmong: (paths: string[]) => invoke<boolean[]>("folders_among", { paths }),
  prepareExample: (path: string) => invoke<void>("prepare_example", { path }),
  exampleProgress: (paths: string[]) => invoke<(ExampleProgress | null)[]>("example_progress", { paths }),
  openMain: (screen: "index" | "settings") => invoke<void>("open_main", { screen }),
  setWidgetBox: (left: number, top: number, right: number, bottom: number) => invoke<void>("set_widget_box", { left, top, right, bottom }),
  widgetFillsScreen: () => invoke<boolean>("widget_fills_screen"),
  showMain: () => invoke<void>("show_main"),
};

export function previewUrl(path: string, box?: PreviewBox, scale = 1): string {
  const url = convertFileSrc(path, "thumb");
  if (!box) return url;
  const { width, height } = previewPixels(box, scale);
  return `${url}?w=${width}&h=${height}&fit=${box.fit}`;
}

export function mediaUrl(path: string): string {
  return convertFileSrc(path);
}

export function largePreviewUrl(path: string, size: number, page = 1): string {
  return `${convertFileSrc(path, "thumb")}?large=${Math.round(size)}${page > 1 ? `&page=${page}` : ""}`;
}

export function previewPixels(box: PreviewBox, scale: number): { width: number; height: number } {
  const pixels = (n: number) => Math.max(1, Math.round(n * scale));
  return { width: pixels(box.width), height: pixels(box.height) };
}

export interface FolderPreviews {
  shape: number;
  order: readonly number[] | null;
  width: number;
  height: number;
}

export interface PreviewBox {
  width: number;
  height: number;
  fit: "cover" | "contain";
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

export interface ExampleProgress {
  made: number;
  of: number;
  reading: boolean;
}

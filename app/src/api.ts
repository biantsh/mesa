import { convertFileSrc, invoke } from "@tauri-apps/api/core";

export type Kind = "drive" | "folder" | "file";
export type Category = "image" | "video" | "audio" | "document";
export type IndexStatus = "notIndexed" | "indexing" | "indexed";

export interface Entry {
  name: string;
  path: string;
  kind: Kind;
  category: Category | null;
  readable: boolean;
  hasContents: boolean;
  index: IndexStatus;
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

export const COMPUTER = "";

export const api = {
  homeDir: () => invoke<string>("home_dir"),
  listDir: (path: string) => invoke<Listing>("list_dir", { path }),
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

import type { Category } from "./api";

export const TYPE_ORDER: Category[] = ["image", "document", "video", "audio"];
export const DEFAULT_TYPES: Category[] = ["image", "document"];

export const TYPE_NAMES: Record<Category, string> = {
  image: "Images",
  document: "Documents",
  video: "Videos",
  audio: "Audio",
};

const TYPE_WORDS: Record<Category, string> = {
  image: "images",
  document: "documents",
  video: "videos",
  audio: "audio",
};

export function typesToIndex(indexed: Category[], picked: Category[], nothingIndexed: boolean): Category[] {
  const chosen = nothingIndexed ? new Set(picked) : new Set([...indexed, ...picked]);
  return TYPE_ORDER.filter((type) => chosen.has(type));
}

export function hasNewTypes(indexed: Category[], picked: Category[]): boolean {
  return picked.some((type) => !indexed.includes(type));
}

export function describeTypes(types: Category[]): string {
  const words = TYPE_ORDER.filter((type) => types.includes(type)).map((type) => TYPE_WORDS[type]);
  if (words.length <= 1) return words.join("");
  return `${words.slice(0, -1).join(", ")} and ${words[words.length - 1]}`;
}

import type { Category } from "./api";

export const TYPE_ORDER: Category[] = ["document", "image", "video", "audio", "code"];
export const DEFAULT_TYPES: Category[] = ["document", "image"];

export const TYPE_NAMES: Record<Category, string> = {
  image: "Images",
  document: "Documents",
  video: "Videos",
  audio: "Audio",
  code: "Code",
};

export function typeOffTip(type: Category): string {
  if (type === "code") return "Code isn't being indexed. Turn it on in the bar above.";
  const kind = type === "audio" ? "Audio files" : TYPE_NAMES[type];
  return `${kind} aren't being indexed. Turn them on in the bar above.`;
}

export const NO_TYPES_TIP = "No file types chosen to index.";

const TYPE_WORDS: Record<Category, string> = {
  image: "images",
  document: "documents",
  video: "videos",
  audio: "audio",
  code: "code",
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

import type { Category } from "./api";

export const EXAMPLE_KINDS: { category: Category; label: string; extensions: string[] }[] = [
  { category: "image", label: "Images", extensions: ["jpg", "jpeg", "jfif", "jpe", "png", "gif", "webp", "bmp", "tif", "tiff", "heic", "heif", "avif", "dng", "cr2", "cr3", "nef", "nrw", "arw", "srf", "sr2", "orf", "rw2", "pef", "raf", "srw"] },
  { category: "video", label: "Videos", extensions: ["mp4", "m4v", "mov", "mkv", "avi", "webm", "3gp", "3g2", "mpg", "mpeg", "mts", "m2ts", "wmv"] },
  { category: "audio", label: "Audio", extensions: ["mp3", "wav", "wave", "flac", "m4a", "m4b", "aac", "aiff", "aif", "ogg", "opus", "caf", "amr", "wma"] },
  { category: "document", label: "Documents", extensions: ["pdf", "txt", "md", "markdown", "docx", "docm", "xlsx", "xlsm", "pptx", "pptm", "ppsx", "odt", "ods", "odp", "pages", "numbers", "key", "rtf", "epub", "dotx", "dotm", "xltx", "xltm", "potx", "potm", "ott", "ots", "otp"] },
];

export const MAX_EXAMPLES = 16;

export const fileNameOf = (path: string) => path.split(/[\\/]/).pop() || path;

const extensionOf = (path: string) => {
  const name = fileNameOf(path);
  return name.includes(".") ? (name.split(".").pop() ?? "").toLowerCase() : "";
};

export function categoryOfPath(path: string): Category | null {
  const extension = extensionOf(path);
  return EXAMPLE_KINDS.find((kind) => kind.extensions.includes(extension))?.category ?? null;
}

export function countByKind(paths: string[]): { category: Category; count: number }[] {
  return EXAMPLE_KINDS.map(({ category }) => ({ category, count: paths.filter((path) => categoryOfPath(path) === category).length })).filter(
    ({ count }) => count > 0,
  );
}

export function distinctNames(paths: string[]): string[] {
  const names = paths.map(fileNameOf);
  return paths.map((path, i) => {
    if (names.filter((name) => name === names[i]).length < 2) return names[i];
    const folder = path.split(/[\\/]/).slice(-2, -1)[0];
    return folder ? `${names[i]} (${folder})` : names[i];
  });
}

export function shortName(name: string, max = 34): string {
  if (name.length <= max) return name;
  const dot = name.lastIndexOf(".");
  const extension = dot > 0 && name.length - dot <= 6 ? name.slice(dot) : "";
  return `${name.slice(0, Math.max(1, max - extension.length - 1))}…${extension}`;
}

export function addExamples(current: string[], picked: string[]): string[] {
  return [...new Set([...current, ...picked])].slice(0, MAX_EXAMPLES);
}

export function leftOut(current: string[], picked: string[]): number {
  const fresh = new Set(picked.filter((path) => !current.includes(path)));
  return Math.max(0, current.length + fresh.size - MAX_EXAMPLES);
}

export function limitNotice(left: number): string {
  return `${left} left out (${MAX_EXAMPLES} at most)`;
}

export function dropHint(supported: string[], unsupported: string[] = [], folders: string[] = [], room = MAX_EXAMPLES): string | null {
  const kinds = countByKind(supported);
  const total = kinds.reduce((sum, kind) => sum + kind.count, 0);
  if (total > 0 && room <= 0) return `Only ${MAX_EXAMPLES} files can be searched with at once`;
  if (total === 0) {
    if (unsupported.length === 0 && folders.length === 0) return null;
    if (unsupported.length === 0) return folders.length === 1 ? "Can't search with a folder" : "Can't search with folders";
    if (folders.length > 0) return "Can't search with these files or folders";
    const extensions = new Set(unsupported.map(extensionOf).filter(Boolean));
    if (extensions.size === 1) return `Can't search with .${[...extensions][0]} files`;
    return unsupported.length === 1 ? "Can't search with this file" : "Can't search with these files";
  }
  const hint = kinds.length > 1 ? "Drop to search with your files" : `Drop to search with your ${kinds[0].category}${total > 1 ? "s" : ""}`;
  const skipped = unsupported.length + folders.length;
  const over = Math.max(0, total - room);
  if (over > 0) return `${hint} (${over} over the limit of ${MAX_EXAMPLES})`;
  return skipped > 0 ? `${hint} (${skipped} skipped)` : hint;
}

export function dropIcon(
  dragged: { supported: string[]; unsupported: string[]; folders?: string[] } | null,
  room = MAX_EXAMPLES,
): Category | "files" | "refuse" | "search" {
  if (!dragged || dragged.supported.length + dragged.unsupported.length + (dragged.folders?.length ?? 0) === 0) return "search";
  const kinds = countByKind(dragged.supported);
  if (kinds.length === 0 || room <= 0) return "refuse";
  return kinds.length > 1 ? "files" : kinds[0].category;
}

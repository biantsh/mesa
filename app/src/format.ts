import type { Entry } from "./api";

const UNITS = ["B", "KB", "MB", "GB", "TB"];
const decimals = new Intl.NumberFormat(undefined, { maximumFractionDigits: 1 });
const whole = new Intl.NumberFormat(undefined, { maximumFractionDigits: 0 });
const when = new Intl.DateTimeFormat(undefined, { dateStyle: "medium", timeStyle: "short" });

export function formatSize(bytes: number, precise = false): string {
  let value = bytes;
  let unit = 0;
  while (value >= 1024 && unit < UNITS.length - 1) {
    value /= 1024;
    unit++;
  }
  return `${((value < 10 || precise) && unit > 0 ? decimals : whole).format(value)} ${UNITS[unit]}`;
}

export function formatModified(milliseconds: number): string {
  return when.format(new Date(milliseconds));
}

const CATEGORIES = { image: "Image", video: "Video", audio: "Audio", document: "Document" } as const;

export function typeLabel(entry: Entry): string {
  if (entry.kind === "drive") return "Drive";
  if (entry.kind === "folder") return "Folder";
  return entry.category ? CATEGORIES[entry.category] : "File";
}

const counts = new Intl.NumberFormat();

export function describeIndexTotals(files: number, bytes: number): string {
  return `${counts.format(files)} ${files === 1 ? "file" : "files"} indexed (${files === 0 ? "0 MB" : formatSize(bytes, true)})`;
}

function plural(count: number, one: string): string {
  return `${count} ${count === 1 ? one : `${one}s`}`;
}

export function describeItems(entries: Entry[]): string {
  const files = entries.filter((entry) => entry.kind === "file").length;
  const folders = entries.length - files;
  const total = plural(entries.length, "item");
  return files > 0 && folders > 0
    ? `${total} (${plural(folders, "folder")}, ${plural(files, "file")})`
    : total;
}

export function describeIndexed(entries: Entry[], selection = false): string | null {
  const possible = entries.filter(
    (entry) => entry.readable && entry.index !== "unindexable" && entry.index !== "excluded",
  );
  if (possible.length === 0) return null;
  const indexed = possible.filter((entry) => entry.index === "indexed").length;
  return `${indexed} of ${possible.length} ${selection ? "selected " : ""}indexed`;
}

const downloadedFormat = new Intl.NumberFormat(undefined, { minimumFractionDigits: 1, maximumFractionDigits: 1 });
const totalFormat = new Intl.NumberFormat(undefined, { maximumFractionDigits: 0 });

export function describeDownload(done: number, total: number): string {
  const percent = total > 0 ? Math.min(100, Math.floor((done / total) * 100)) : 0;
  return `${downloadedFormat.format(done / 1e6)} of ${totalFormat.format(total / 1e6)} MB (${percent}%)`;
}

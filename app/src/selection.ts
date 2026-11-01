import type { Entry } from "./api";

export function clicked(current: ReadonlySet<string>, path: string, toggle: boolean): Set<string> {
  if (!toggle) return new Set([path]);
  const next = new Set(current);
  if (!next.delete(path)) next.add(path);
  return next;
}

export function everything(entries: Entry[]): Set<string> {
  return new Set(entries.filter((entry) => entry.readable).map((entry) => entry.path));
}

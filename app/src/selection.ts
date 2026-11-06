import type { Entry } from "./api";

export const SELECTING_FROM = 2;

export function clicked(current: ReadonlySet<string>, path: string, toggle: boolean): Set<string> {
  if (!toggle) return new Set([path]);
  const next = new Set(current);
  if (!next.delete(path)) next.add(path);
  return next;
}

export function everything(entries: Entry[]): Set<string> {
  return new Set(entries.filter((entry) => entry.readable).map((entry) => entry.path));
}

export interface Picked {
  paths: ReadonlySet<string>;
  anchor: string | null;
  active: string | null;
}

export const NOTHING_PICKED: Picked = { paths: new Set(), anchor: null, active: null };

export type Pick = "replace" | "toggle" | "range" | "addRange";

export function howToPick(toggle: boolean, range: boolean): Pick {
  return range ? (toggle ? "addRange" : "range") : toggle ? "toggle" : "replace";
}

export function pick(current: Picked, order: readonly string[], path: string, how: Pick): Picked {
  if (how === "range" || how === "addRange") {
    const anchor = current.anchor ?? current.active ?? path;
    const covered = span(order, anchor, path);
    const paths = how === "addRange" ? new Set([...current.paths, ...covered]) : covered;
    return { paths, anchor, active: path };
  }
  const paths = how === "toggle" ? clicked(current.paths, path, true) : new Set([path]);
  return { paths, anchor: path, active: path };
}

function span(order: readonly string[], from: string, to: string): Set<string> {
  const a = order.indexOf(from);
  const b = order.indexOf(to);
  if (a < 0 || b < 0) return new Set([to]);
  return new Set(order.slice(Math.min(a, b), Math.max(a, b) + 1));
}

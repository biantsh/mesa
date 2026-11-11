import type { Entry } from "./api";

export interface Shape {
  rust: number | null;
  order: readonly number[] | null;
}

const shapes = new WeakMap<readonly object[], Shape>();

export function shapeOf(entries: readonly object[]): Shape {
  let shape = shapes.get(entries);
  if (!shape) {
    shape = { rust: null, order: null };
    shapes.set(entries, shape);
  }
  return shape;
}

export function withShape<T extends readonly Entry[]>(entries: T, shape: Shape): T {
  shapes.set(entries, shape);
  return entries;
}

export function sameShape<T extends readonly Entry[]>(next: T, was: readonly Entry[]): T {
  return withShape(next, shapeOf(was));
}

const places = new WeakMap<Shape, Map<string, number>>();
const NONE: Entry[] = [];

export function placesOf(entries: readonly { path: string }[]): Map<string, number> {
  const shape = shapeOf(entries);
  let found = places.get(shape);
  if (!found) {
    found = new Map(entries.map((entry, at) => [entry.path, at]));
    places.set(shape, found);
  }
  return found;
}

export function entriesWith(entries: readonly Entry[], paths: ReadonlySet<string>): Entry[] {
  if (paths.size === 0) return NONE;
  if (paths.size > entries.length / 8) return entries.filter((entry) => paths.has(entry.path));
  const at = placesOf(entries);
  const found: number[] = [];
  for (const path of paths) {
    const place = at.get(path);
    if (place !== undefined) found.push(place);
  }
  return found.sort((a, b) => a - b).map((place) => entries[place]);
}

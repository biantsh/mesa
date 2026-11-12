import type { Category, Entry, IndexStatus, Kind, Listed, PrivacyPane } from "./api";

export interface Columns {
  names: string[];
  prefix: string | null;
  paths: string[] | null;
  kinds: string;
  categories: string;
  statuses: string;
  flags: string;
  sizes: (number | null)[];
  modified: (number | null)[];
  privacy: [number, PrivacyPane][];
  legend: { kinds: Kind[]; categories: Category[]; statuses: IndexStatus[] };
}

const LETTERS = "0123456789abcdefghijklmnopqrstuv";
const READABLE = 1;
const HAS_CONTENTS = 2;
const QUEUED = 4;
const EXCLUDED_HERE = 8;
const REMOVABLE = 16;

const codeOf = (letter: string) => LETTERS.indexOf(letter);

export function entriesOf(columns: Columns): Entry[] {
  const { names, prefix, paths, kinds, categories, statuses, flags, sizes, modified, privacy, legend } = columns;
  const panes = new Map(privacy);
  const entries: Entry[] = new Array(names.length);
  for (let i = 0; i < names.length; i++) {
    const bits = codeOf(flags[i]);
    entries[i] = {
      name: names[i],
      path: paths ? paths[i] : `${prefix}${names[i]}`,
      kind: legend.kinds[codeOf(kinds[i])],
      category: legend.categories[codeOf(categories[i])] ?? null,
      readable: (bits & READABLE) !== 0,
      hasContents: (bits & HAS_CONTENTS) !== 0,
      index: legend.statuses[codeOf(statuses[i])],
      queued: (bits & QUEUED) !== 0,
      excludedHere: (bits & EXCLUDED_HERE) !== 0,
      removable: (bits & REMOVABLE) !== 0,
      size: sizes[i],
      modified: modified[i],
      privacy: panes.get(i) ?? null,
    };
  }
  return entries;
}

export interface ListedColumns extends Columns {
  missing: string;
  slots: number[];
}

export function listedOf(columns: ListedColumns): Listed[] {
  return entriesOf(columns).map((entry, i) => ({ ...entry, missing: columns.missing[i] === "1", slot: columns.slots[i] }));
}

export type Run = [number, number] | number;

export function applyRuns(entries: readonly Entry[], runs: Run[], fresh: Entry[]): Entry[] | null {
  const made: Entry[] = [];
  let next = 0;
  for (const run of runs) {
    if (typeof run === "number") {
      if (next + run > fresh.length) return null;
      for (let i = 0; i < run; i++) made.push(fresh[next++]);
    } else {
      const [from, count] = run;
      if (from + count > entries.length) return null;
      for (let i = from; i < from + count; i++) made.push(entries[i]);
    }
  }
  return next === fresh.length ? made : null;
}

export type StatusChange = [number, IndexStatus, boolean, boolean, boolean];

export function withStatusChanges(entries: readonly Entry[], changes: StatusChange[]): Entry[] | null {
  if (changes.length === 0) return null;
  const next = entries.slice();
  for (const [at, index, queued, excludedHere, removable] of changes) {
    const entry = next[at];
    if (!entry) return null;
    next[at] = { ...entry, index, queued, excludedHere, removable };
  }
  return next;
}

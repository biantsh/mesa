import { api, type MatchDetails, type SearchHit } from "./api";

export function wantsDetails(hit: SearchHit): boolean {
  if (hit.reason === "words") return hit.category === "image" || hit.category === "document";
  return hit.category === "document" && hit.at !== null;
}

const lookedFor = (hit: SearchHit, query: string) => (hit.reason === "words" ? query : "");

interface Kept {
  promise: Promise<MatchDetails | null>;
  value?: MatchDetails | null;
}

const MOST = 200;
const kept = new Map<string, Kept>();

const keyOf = (hit: SearchHit, query: string) => `${hit.path}\n${hit.modified}\n${hit.at ?? ""}\n${lookedFor(hit, query)}`;

export function detailsFor(hit: SearchHit, query: string): Promise<MatchDetails | null> {
  const key = keyOf(hit, query);
  const known = kept.get(key);
  if (known) return known.promise;
  if (kept.size >= MOST) kept.delete(kept.keys().next().value!);
  const entry: Kept = {
    promise: api.matchDetails(hit.path, lookedFor(hit, query), hit.at).then(
      (found) => (entry.value = found),
      () => (entry.value = null),
    ),
  };
  kept.set(key, entry);
  return entry.promise;
}

export function detailsNow(hit: SearchHit, query: string): MatchDetails | null | undefined {
  return kept.get(keyOf(hit, query))?.value;
}

export function detailsAsked(hit: SearchHit, query: string): boolean {
  return kept.has(keyOf(hit, query));
}

export function prefetchDetails(hit: SearchHit, query: string, pressed: boolean) {
  if (!wantsDetails(hit) || (hit.category === "document" && !pressed)) return;
  void detailsFor(hit, query);
}

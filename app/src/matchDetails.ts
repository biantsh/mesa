import { api, type MatchDetails, type SearchHit } from "./api";

export function wantsDetails(hit: SearchHit): boolean {
  if (hit.category === "code") return true;
  if (hit.reason === "words") return hit.byWords !== "name" && (hit.category === "image" || hit.category === "document");
  return hit.category === "document" && hit.at !== null;
}

const lookedFor = (hit: SearchHit, query: string) => (hit.reason === "words" || hit.category === "code" ? query : "");

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
    promise: api
      .matchDetails(hit.path, lookedFor(hit, query), hit.at)
      .then((found) => withAlsoIn(hit, found))
      .then(
        (found) => (entry.value = found),
        () => (entry.value = null),
      ),
  };
  kept.set(key, entry);
  return entry.promise;
}

const ALSO_IN_WAIT_MS = 300;

async function withAlsoIn(hit: SearchHit, found: MatchDetails): Promise<MatchDetails> {
  if (hit.category !== "code" || !found.code) return found;
  const late = new Promise<undefined>((resolve) => setTimeout(resolve, ALSO_IN_WAIT_MS));
  const alsoIn = await Promise.race([api.alsoIn(hit.path, found.code.start).catch(() => []), late]);
  return alsoIn === undefined ? found : { ...found, alsoIn };
}

export function detailsNow(hit: SearchHit, query: string): MatchDetails | null | undefined {
  return kept.get(keyOf(hit, query))?.value;
}

export function detailsAsked(hit: SearchHit, query: string): boolean {
  return kept.has(keyOf(hit, query));
}

const POINTED_AT_MS = 120;
let pointedAt = 0;

export function prefetchDetails(hit: SearchHit, query: string, pressed: boolean) {
  window.clearTimeout(pointedAt);
  if (!wantsDetails(hit) || (hit.category === "document" && !pressed)) return;
  if (pressed) void detailsFor(hit, query);
  else pointedAt = window.setTimeout(() => void detailsFor(hit, query), POINTED_AT_MS);
}

export function codeLine(hit: SearchHit): number {
  const prefix = `${hit.path}\n${hit.modified}\n`;
  for (const [key, entry] of kept) {
    if (key.startsWith(prefix) && entry.value?.code) return entry.value.code.start;
  }
  return hit.at ?? 1;
}

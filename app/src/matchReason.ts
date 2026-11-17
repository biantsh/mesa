import type { SearchHit } from "./api";
import { atIsPage, pagingOf } from "./paging";

export interface MatchReason {
  kind: "words" | "meaning" | "filters";
  text: string;
  detail?: string;
  faint?: boolean;
}

export interface Found {
  hit: Pick<SearchHit, "reason" | "byWords" | "at" | "category" | "name">;
  query: string;
  withFiles: boolean;
  files?: string[];
  weak: boolean;
}

export function formatMoment(milliseconds: number): string {
  const seconds = Math.floor(milliseconds / 1000);
  const [h, m, s] = [Math.floor(seconds / 3600), Math.floor(seconds / 60) % 60, seconds % 60];
  const two = (n: number) => String(n).padStart(2, "0");
  return h > 0 ? `${h}:${two(m)}:${two(s)}` : `${m}:${two(s)}`;
}

export function quotedWords(query: string, how: SearchHit["byWords"]): string {
  const words = query.trim().split(/\s+/).filter(Boolean);
  if (how === "phrase" || words.length <= 1) return `“${query.trim()}”`;
  return words.map((word) => `“${word}”`).join(", ");
}

function nameHasAsTyped(name: string, query: string): boolean {
  const fold = (text: string) => text.normalize("NFD").replace(/\p{M}/gu, "").toLowerCase();
  return fold(name).includes(fold(query.trim()));
}

export function matchReason({ hit, query, withFiles, weak, files = [] }: Found): MatchReason {
  const paging = pagingOf(hit.name);
  const where =
    hit.at === null
      ? undefined
      : paging && atIsPage(hit.name)
        ? `Best match on ${paging} ${hit.at}`
        : (hit.category === "video" || hit.category === "audio") && hit.at > 0
          ? `Best match at ${formatMoment(hit.at)}`
          : undefined;
  switch (hit.reason) {
    case "words":
      if (hit.byWords === "name") return { kind: "words", text: `Name contains ${quotedWords(query, nameHasAsTyped(hit.name, query) ? "phrase" : "words")}` };
      return { kind: "words", text: `Contains ${quotedWords(query, hit.byWords)}` };
    case "meaning":
      return { kind: "meaning", text: "Closely matches your query", detail: where };
    case "fewFiles":
      return { kind: "filters", text: "One of the few files matching your filters" };
    case "clues":
      if (files.length > 0) return { kind: "words", text: `Contains text from the ${files.length === 1 ? "file" : "files"} you searched with` };
      return { kind: "words", text: "Named in your search" };
  }
  if (weak) return { kind: "meaning", text: "Loosely matches your query", detail: where, faint: true };
  if (!query && !withFiles) return { kind: "filters", text: "Matches the filters you chose" };
  return { kind: "meaning", text: "Closely matches your query", detail: where };
}

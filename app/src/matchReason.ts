import type { SearchHit } from "./api";

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

export function matchReason({ hit, query, withFiles, weak }: Found): MatchReason {
  const pdf = hit.name.toLowerCase().endsWith(".pdf");
  const where =
    hit.at === null
      ? undefined
      : pdf
        ? `Best match on page ${hit.at}`
        : (hit.category === "video" || hit.category === "audio") && hit.at > 0
          ? `Best match at ${formatMoment(hit.at)}`
          : undefined;
  switch (hit.reason) {
    case "words":
      return { kind: "words", text: `Contains ${quotedWords(query, hit.byWords)}` };
    case "meaning":
      return { kind: "meaning", text: "Closely matches your query", detail: where };
    case "fewFiles":
      return { kind: "filters", text: "One of the few files matching your filters" };
  }
  if (weak) return { kind: "meaning", text: "Loosely matches your query", detail: where, faint: true };
  if (!query && !withFiles) return { kind: "filters", text: "Matches the filters you chose" };
  return { kind: "meaning", text: "Closely matches your query", detail: where };
}

import type { CodePart, CodeView, MatchSnippet, SearchHit } from "../../../app/src/api";
import { keyLabel, modifierLabels } from "../../../app/src/hotkey";
import { isMac, platform } from "../../../app/src/platform";
import { GITHUB } from "../links";
import evaluatePy from "./code/iris-classifier/evaluate.py?raw";
import { highlightPython } from "./python";

type Category = SearchHit["category"];

const HOME = "/Users/demo";

interface DemoFile {
  path: string;
  category: Category;
  date: [number, number, number];
  size: number;
  at?: number;
  picture?: string;
  slides?: string[];
  text?: string[];
  source?: string;
  project?: string;
}

const LIBRARY: DemoFile[] = [
  { path: `${HOME}/Pictures/DSC_0187.jpg`, category: "image", date: [2024, 8, 14], size: 6_120_000, picture: "beach-dog-1.webp" },
  { path: `${HOME}/Pictures/DSC_0191.jpg`, category: "image", date: [2024, 8, 14], size: 5_870_000, picture: "beach-dog-2.webp" },
  { path: `${HOME}/Pictures/IMG_2231.jpg`, category: "image", date: [2024, 7, 2], size: 4_310_000, picture: "beach.webp" },
  { path: `${HOME}/Pictures/IMG_1984.jpg`, category: "image", date: [2024, 5, 19], size: 3_980_000, picture: "park-dog.webp" },
  { path: `${HOME}/Pictures/IMG_1702.jpg`, category: "image", date: [2024, 3, 9], size: 3_420_000, picture: "garden.webp" },
  { path: `${HOME}/Movies/Holiday day 2.mov`, category: "video", date: [2024, 8, 15], size: 412_000_000, at: 102_000, picture: "beach-video.webp" },
  { path: `${HOME}/Pictures/IMG_0412.jpg`, category: "image", date: [2025, 6, 1], size: 4_020_000, picture: "beach-dog-3.webp" },
  { path: `${HOME}/Pictures/IMG_2240.jpg`, category: "image", date: [2024, 7, 3], size: 4_120_000, picture: "waves.webp" },
  { path: `${HOME}/Pictures/IMG_2251.jpg`, category: "image", date: [2024, 7, 3], size: 3_870_000, picture: "dog-swim.webp" },
  { path: `${HOME}/Pictures/DSC_0203.jpg`, category: "image", date: [2024, 8, 12], size: 5_990_000, picture: "dog-ball.webp" },
  { path: `${HOME}/Movies/Puppy.mov`, category: "video", date: [2023, 5, 21], size: 188_000_000, at: 14_000, picture: "park-video.webp" },
  { path: `${HOME}/Pictures/IMG_0877.jpg`, category: "image", date: [2023, 9, 10], size: 3_210_000, picture: "paw-prints.webp" },
  { path: `${HOME}/Pictures/IMG_3310.heic`, category: "image", date: [2025, 8, 2], size: 2_640_000, picture: "sunset.webp" },
  { path: `${HOME}/Pictures/IMG_3322.heic`, category: "image", date: [2025, 8, 2], size: 2_710_000, picture: "beach-dog-4.webp" },
  { path: `${HOME}/Movies/Coast walk.mp4`, category: "video", date: [2025, 4, 12], size: 264_000_000, at: 61_000, picture: "cliffs.webp" },
  { path: `${HOME}/Pictures/IMG_1105.jpg`, category: "image", date: [2023, 6, 18], size: 3_540_000, picture: "lake-dog.webp" },
  { path: `${HOME}/Pictures/DSC_0150.jpg`, category: "image", date: [2024, 8, 13], size: 5_430_000, picture: "umbrella.webp" },
  { path: `${HOME}/Pictures/IMG_0901.jpg`, category: "image", date: [2023, 9, 11], size: 2_980_000, picture: "sandcastle.webp" },
  { path: `${HOME}/Movies/Drone beach.mp4`, category: "video", date: [2024, 8, 17], size: 342_000_000, at: 8_000, picture: "aerial.webp" },
  {
    path: `${HOME}/Documents/Work/Q3 2026 Business Review.pptx`,
    category: "document",
    date: [2026, 10, 13],
    size: 8_640_000,
    picture: "q3-review/",
    slides: [
      "Larkspur Coffee Roasters. Q3 2026 Business Review. Leadership team, October 14, 2026",
      "Agenda. Q3 at a glance: revenue, subscribers and margin. Subscriptions: churn, pauses and delivery options. Wholesale & cafés: new partners by region. New roasts: what launched and how it sold. Q4 priorities: gift season, the app and a second roaster",
      "Q3 at a glance. Revenue $4.2M, up 18% vs Q3 2025. Active subscribers 31,400, up 2,900 this quarter. Gross margin 41.3%, up 1.8 points. Net Promoter Score 62, up 5 points. Our best quarter since we opened, and the first with every channel growing",
      "Subscription churn fell to 3.1%. Monthly churn: the share of active subscribers who cancelled. April 4.9%, May 4.6%, June 4.6%, July 4.0%, August 3.5%, September 3.1%. Changes shipped July 1. What changed: pause instead of cancel, 1 in 3 who start to cancel now pause. Flexible delivery every 2, 3 or 4 weeks, changed any time. Roaster’s choice box re-launched in July: 4,100 subscribers. Every point of churn we cut is worth about $310K a year in revenue",
      "Wholesale up 24%, led by 18 new cafés. New café partners by region: Portland 8, Seattle 5, Bay Area 3, Boise 2. 62 wholesale accounts. $1.1M wholesale revenue in Q3. 96% of cafés reordered",
      "Ethiopia Guji sold out in 9 days. Ethiopia Guji, natural process: blueberry, jasmine, cacao nib; 2,400 bags, sold out in 9 days. Autumn Blend, espresso blend: dark chocolate, molasses, orange; our best-selling espresso by week 3. Decaf Huila, Colombia, Swiss Water: toffee, red apple, brown sugar; now 9% of subscription boxes",
      "Q4 priorities. Holiday gift boxes: 12,000 boxes in three sizes, corporate orders open Nov 3, last shipping day Dec 12. Subscriber app: skip, swap and pause on the phone, beta with 500 subscribers in November, launch target December 1. Second roaster: 35 kg roaster in our Salem site, doubles capacity to 1.2 t a week, commissioned by November 20",
      "Thank you. Questions and discussion. Next review: January 13, 2027",
    ],
  },
  {
    path: `${HOME}/Desktop/confusion_matrix.png`,
    category: "image",
    date: [2026, 11, 12],
    size: 41_300,
   
    picture: "charts/confusion-matrix.svg",
    text: ["Confusion matrix: logistic regression", "setosa", "versicolor", "virginica", "Predicted label", "True label", "15 0 0", "0 14 1", "0 2 13"],
  },
  {
    path: `${HOME}/Projects/iris-classifier/evaluate.py`,
    category: "code",
    date: [2026, 11, 12],
    size: evaluatePy.length,
   
    source: evaluatePy,
    project: "iris-classifier",
  },
  { path: `${HOME}/Projects/iris-classifier/figures/confusion_matrix.png`, category: "image", date: [2026, 11, 12], size: 41_300, picture: "charts/confusion-matrix.svg" },
  { path: `${HOME}/Projects/iris-classifier/figures/learning_curve.png`, category: "image", date: [2026, 11, 12], size: 52_800, picture: "charts/learning-curve.svg" },
];

function vectorOf(base64: string): Float32Array {
  const vector = Float32Array.from(new Int8Array(Uint8Array.from(atob(base64), (char) => char.charCodeAt(0)).buffer));
  const length = Math.hypot(...vector) || 1;
  return vector.map((one) => one / length);
}

const cosine = (a: Float32Array, b: Float32Array) => a.reduce((sum, one, i) => sum + one * b[i], 0);

interface Index {
  files: Map<string, { at: number; vector: Float32Array }[]>;
  queries: Map<string, Float32Array>;
  background: { modified: number; vector: Float32Array }[];
}
let loading: Promise<Index> | undefined;
const loadIndex = () =>
  (loading ??= fetch("/demo/index.json")
    .then((response) => response.json())
    .then(
      ({ files, queries, background }: { files: Record<string, { at: number; vector: string }[]>; queries: Record<string, string>; background: Record<string, string[]> }) => ({
        files: new Map(Object.entries(files).map(([path, parts]) => [path, parts.map(({ at, vector }) => ({ at, vector: vectorOf(vector) }))])),
        queries: new Map(Object.entries(queries).map(([text, vector]) => [text, vectorOf(vector)])),
        background: Object.entries(background).flatMap(([year, vectors]) =>
          vectors.map((vector) => ({ modified: new Date(Number(year), 6, 1, 12).getTime(), vector: vectorOf(vector) })),
        ),
      }),
    ));

function closeness(index: Index, file: DemoFile, query: Float32Array): { score: number; at: number | null } {
  let best = { score: -1, at: null as number | null };
  for (const part of index.files.get(file.path) ?? []) {
    const score = cosine(part.vector, query);
    if (score > best.score) best = { score, at: file.category === "video" ? (file.at ?? null) : part.at };
  }
  return best;
}

interface Scored {
  file: DemoFile | null;
  kind: Category;
  score: number;
  at: number | null;
  words: boolean;
}

const CLOSE = 2.5;
const CUTOFFS: [number, number][] = [[1_000, 3.0], [5_000, 3.5], [30_000, 4.0], [300_000, 4.5], [3_000_000, 5.0]];
const MIN_FILES = 30;
const PRIOR_FILES = 30;
const MIN_SPREAD = 0.01;
const MAD_TO_SPREAD = 1.4826;
const ON_THE_LINE = 30;
const PER_FOLD = 0.75;

function median(values: number[]): number {
  const sorted = [...values].sort((a, b) => a - b);
  const middle = sorted.length >> 1;
  return sorted.length % 2 ? sorted[middle] : (sorted[middle - 1] + sorted[middle]) / 2;
}

function usual(scores: number[]) {
  const middle = median(scores);
  const spread = median(scores.map((score) => Math.abs(score - middle))) * MAD_TO_SPREAD;
  return { count: scores.length, middle, variance: spread * spread };
}

function rank(scored: Scored[], among: Scored[]): { ranked: (Scored & { reason: SearchHit["reason"] })[]; close: number } {
  const byKind = new Map<Category, number[]>();
  for (const one of among) {
    const scores = byKind.get(one.kind);
    if (scores) scores.push(one.score);
    else byKind.set(one.kind, [one.score]);
  }
  const kinds = new Map([...byKind].map(([kind, scores]) => [kind, usual(scores)]));
  const all = usual(among.map((one) => one.score));
  let spread = 0;
  let counted = 0;
  for (const kind of kinds.values()) if (kind.count >= 2) [spread, counted] = [spread + kind.variance * kind.count, counted + kind.count];
  const shared = counted > 0 ? spread / counted : 0;
  const line = CUTOFFS.findLast(([from]) => scored.length >= from)?.[1] ?? CLOSE;
  const few = scored.length < MIN_FILES;
  const margin = PER_FOLD * Math.log(ON_THE_LINE / Math.max(1, scored.filter((one) => one.words).length));
  const ranked = scored
    .map((one) => {
      const own = kinds.get(one.kind)!;
      const middle = own.count >= 2 ? own.middle : all.middle;
      const variance = (own.variance * own.count + PRIOR_FILES * shared) / (own.count + PRIOR_FILES);
      const z = (one.score - middle) / Math.max(Math.sqrt(variance), MIN_SPREAD);
      const words = one.words ? line + margin : -Infinity;
      const reason: SearchHit["reason"] = words >= z ? "words" : z >= line ? "meaning" : few ? "fewFiles" : null;
      return { ...one, reason, z: Math.max(z, words) };
    })
    .sort((a, b) => b.z - a.z);
  return { ranked, close: few ? ranked.length : ranked.filter((one) => one.z >= line).length };
}

function found(index: Index, query: Float32Array, filter: Filter, text: string, leftOut: string[] = []): { hits: SearchHit[]; close: number } {
  const all: (Scored & { kept: boolean })[] = [
    ...LIBRARY.map((file) => ({
      file,
      kind: file.category,
      ...closeness(index, file, query),
      words: slideWith(file, text) !== null,
      kept: passes(filter)(file) && !leftOut.includes(file.path),
    })),
    ...index.background.map(({ modified, vector }) => ({
      file: null,
      kind: "image" as const,
      score: cosine(vector, query),
      at: null,
      words: false,
      kept: keeps(filter, "image", modified),
    })),
  ];
  const { ranked, close } = rank(
    all.filter((one) => one.kept),
    all,
  );
  const hits = ranked.flatMap(({ file, score, at, reason }) =>
    file ? [hitOf(file, { score, reason, byWords: reason === "words" ? "phrase" : null, at: reason === "words" ? slideWith(file, text) : at })] : [],
  );
  return { hits, close: ranked.slice(0, close).filter((one) => one.file).length };
}

function modified(file: DemoFile): number {
  const [year, month, day] = file.date;
  return new Date(year, month - 1, day, 12).getTime();
}

function phraseOf(text: string): RegExp | null {
  const words = text.trim().split(/\s+/).filter(Boolean);
  if (words.join("").length < 3) return null;
  const escaped = words.map((word) => word.replace(/[.*+?^${}()|[\]\\]/g, "\\$&"));
  return new RegExp(`(?<![\\p{L}\\p{N}])${escaped.join("\\s+")}`, "iu");
}

function slideWith(file: DemoFile, text: string): number | null {
  const phrase = file.slides && phraseOf(text);
  if (!phrase) return null;
  const slide = file.slides!.findIndex((one) => phrase.test(one));
  return slide < 0 ? null : slide + 1;
}

function snippetsOf(file: DemoFile, text: string): MatchSnippet[] {
  const phrase = file.slides && phraseOf(text);
  if (!phrase) return [];
  return file.slides!.flatMap((slide, i) => {
    const found = phrase.exec(slide);
    if (!found) return [];
    const start = found.index;
    const end = start + found[0].length;
    const from = start <= 60 ? 0 : slide.indexOf(" ", start - 60) + 1;
    const to = slide.length - end <= 110 ? slide.length : slide.lastIndexOf(" ", end + 110);
    return [
      {
        page: i + 1,
        parts: [
          { text: `${from > 0 ? "…" : ""}${slide.slice(from, start)}`, hit: false },
          { text: found[0], hit: true },
          { text: `${slide.slice(end, to)}${to < slide.length ? "…" : ""}`, hit: false },
        ].filter((part) => part.text !== ""),
      },
    ];
  });
}

type Filter = { category?: Category | null; from?: number | null; to?: number | null };
const keeps = (filter: Filter, category: Category, changed: number) =>
  (!filter.category || category === filter.category) && (filter.from == null || changed >= filter.from) && (filter.to == null || changed < filter.to);
const passes = (filter: Filter) => (file: DemoFile) => keeps(filter, file.category, modified(file));

function search(index: Index, text: string, filter: Filter): { hits: SearchHit[]; close: number } {
  const query = index.queries.get(text.trim());
  if (query) return found(index, query, filter, text);
  const hits = LIBRARY.filter(passes(filter))
    .sort((a, b) => modified(b) - modified(a))
    .map((file) => hitOf(file, { score: 0, reason: null, byWords: null, at: null }));
  return { hits, close: hits.length };
}

function hitOf(file: DemoFile, found: Pick<SearchHit, "score" | "reason" | "byWords" | "at">): SearchHit {
  const parts = file.path.split("/");
  return {
    path: file.path,
    name: parts[parts.length - 1],
    folder: parts[parts.length - 2],
    category: file.category,
    modified: modified(file),
    size: file.size,
    ...found,
    copies: [],
  };
}

const wordsOf = (text: string): string[] => text.toLowerCase().match(/[\p{L}\p{N}]+/gu) ?? [];

function cluesOf(files: DemoFile[]): string[][] {
  const lines = files.flatMap((file) => [...(file.text ?? []), file.path.split("/").pop()!]);
  const clues = new Map<string, string[]>();
  for (const line of lines) {
    const words = wordsOf(line).filter((word) => !/^\d+$/.test(word));
    for (let i = 0; i < words.length; i++) {
      if (words[i].length >= 8) clues.set(words[i], [words[i]]);
      for (const length of [2, 3]) if (i + length <= words.length) clues.set(words.slice(i, i + length).join(" "), words.slice(i, i + length));
    }
  }
  return [...clues.values()];
}

const hasClue = (words: string[], clue: string[]) => words.some((_, i) => clue.every((word, k) => words[i + k] === word));

function piecesOf(source: string): { symbol: string; start: number; end: number }[] {
  const pieces: { symbol: string; start: number; end: number }[] = [];
  source.split("\n").forEach((line, i) => {
    if (!line.trim()) return;
    const opened = line.match(/^(?:async\s+)?(?:def|class)\s+(\w+)/);
    const last = pieces[pieces.length - 1];
    if (opened) pieces.push({ symbol: opened[1], start: i + 1, end: i + 1 });
    else if (last && (/^\s/.test(line) || last.symbol === "")) last.end = i + 1;
    else pieces.push({ symbol: "", start: i + 1, end: i + 1 });
  });
  return pieces;
}

const MIN_CLUES = 2;
const MOST_CLUED = 10;

function codeWithClues(clues: string[][], kept: (file: DemoFile) => boolean): SearchHit[] {
  if (clues.length === 0) return [];
  return LIBRARY.filter((file) => file.source !== undefined && kept(file))
    .map((file) => {
      const words = wordsOf(file.source!);
      const count = clues.filter((clue) => hasClue(words, clue)).length;
      const lines = file.source!.split("\n");
      const best = piecesOf(file.source!)
        .map((piece) => ({ piece, count: clues.filter((clue) => hasClue(wordsOf(lines.slice(piece.start - 1, piece.end).join("\n")), clue)).length }))
        .sort((a, b) => b.count - a.count)[0];
      return { file, count, at: best?.piece.start ?? 1 };
    })
    .filter(({ count }) => count >= MIN_CLUES)
    .sort((a, b) => b.count - a.count)
    .slice(0, MOST_CLUED)
    .map(({ file, count, at }) => hitOf(file, { score: 0.9 + count / 100, reason: "clues", byWords: null, at }));
}

const colourings = new Map<string, CodePart[][]>();
function coloured(source: string): CodePart[][] {
  let lines = colourings.get(source);
  if (!lines) colourings.set(source, (lines = highlightPython(source)));
  return lines;
}

function codeViewOf(file: DemoFile, at: number | null): CodeView {
  const pieces = piecesOf(file.source!);
  const piece = pieces.find((one) => at !== null && at >= one.start && at <= one.end) ?? pieces[0];
  return {
    language: "Python",
    project: file.project ?? null,
    symbol: piece.symbol,
    start: piece.start,
    end: piece.end,
    lines: coloured(file.source!).slice(piece.start - 1, piece.end),
  };
}

const rest = new Map<number, SearchHit[]>();

function similar(index: Index, examples: string[], filter: Filter): { hits: SearchHit[]; close: number } {
  const files = LIBRARY.filter((file) => examples.includes(file.path));
  const clued = codeWithClues(cluesOf(files), passes(filter));
  const parts = files.flatMap((file) => index.files.get(file.path) ?? []);
  const together = parts.reduce((sum, part) => sum.map((one, i) => one + part.vector[i]), new Float32Array(parts[0]?.vector.length ?? 0));
  const length = Math.hypot(...together) || 1;
  const like = found(index, together.map((one) => one / length), filter, "", [...examples, ...clued.map((hit) => hit.path)]);
  return { hits: [...clued, ...like.hits], close: clued.length + like.close };
}

let searches = 0;

declare global {
  interface Window {
    __demoAsked?: { command: string; args: unknown }[];
  }
}

const FILES = new Map(LIBRARY.map((file) => [file.path, file] as const));

export function pictureOf(path: string): string {
  const picture = FILES.get(path)?.picture;
  return picture ? `/demo/files/${picture}` : "/demo/files/none.svg";
}

export const TOTALS = { files: 5_887, bytes: Math.round(60.4 * 1_048_576) };

// The demo's search shortcut, as recorded in its settings.
let searchKey = isMac ? { keys: ["⌥", "Space"], name: "Option+Space" } : { keys: ["Alt", "Space"], name: "Alt+Space" };

const ANSWERS: Record<string, (args: Record<string, unknown>) => unknown> = {
  model_status: () => ({ state: "ready" }),
  index_view: () => ({
    overview: { empty: false, progress: null, everythingIndexed: true, ...TOTALS },
    folder: null,
    lists: null,
  }),
  get_settings: () => ({ gpuAcceleration: true, alwaysFullSpeed: false, startAtSignIn: false }),
  get_search_hotkey: () => ({ active: searchKey, chosenIn: { kind: "mesa" } }),
  set_search_hotkey: (args) => {
    const parts = String(args.shortcut ?? "alt+Space").split("+");
    const code = parts.pop() ?? "Space";
    const held = { ctrl: parts.includes("control"), alt: parts.includes("alt"), shift: parts.includes("shift"), meta: parts.includes("super") };
    if (!held.ctrl && !held.alt && !held.meta) throw { kind: "invalid", message: `${keyLabel(code)} on its own would stop you typing it anywhere.` };
    const keys = [...modifierLabels(held, platform), keyLabel(code)];
    searchKey = { keys, name: keys.join("+") };
    return { active: searchKey, chosenIn: { kind: "mesa" } };
  },
  get_indexed_types: () => ["document", "image", "video", "audio", "code"],
  embedding_gpu: () => ({ name: "Apple M2", shared: true }),
  search: async (args) => {
    const id = ++searches;
    const index = await loadIndex();
    const filter = (args.filter ?? {}) as Filter;
    const files = Array.isArray(args.files) ? (args.files as string[]) : [];
    const { hits, close } = files.length > 0 ? similar(index, files, filter) : search(index, String(args.query ?? ""), filter);
    rest.clear();
    rest.set(id, hits.slice(close));
    return { id, hits: hits.slice(0, close), more: hits.length > close, close: true, moreClose: false };
  },
  search_more: (args) => {
    const id = Number(args.id);
    const hits = rest.get(id) ?? [];
    rest.delete(id);
    return { id, hits, more: false, close: false, moreClose: false };
  },
  match_details: (args) => {
    const file = FILES.get(String(args.path));
    if (file?.source !== undefined) return { snippets: [], closest: null, code: codeViewOf(file, args.at == null ? null : Number(args.at)), alsoIn: [] };
    return { snippets: file ? snippetsOf(file, String(args.query ?? "")) : [], closest: null, code: null, alsoIn: [] };
  },
  code_projects: () => [...new Set(LIBRARY.flatMap((file) => (file.project ? [file.project] : [])))],
  also_in: () => [],
  code_peek: (args) => {
    const source = FILES.get(String(args.path))?.source;
    if (source === undefined) return [];
    const from = Math.max(1, Number(args.line ?? 1));
    return coloured(source).slice(from - 1, from - 1 + 40);
  },
  code_file: (args) => {
    const source = FILES.get(String(args.path))?.source ?? "";
    const lines = coloured(source);
    return { language: "Python", lines, total: lines.length };
  },
  prepare_example: () => null,
  example_progress: (args) => (Array.isArray(args.paths) ? args.paths.map(() => ({ made: 1, of: 1, reading: false })) : []),
  count_with_data: () => 0,
  count_indexed_of_type: () => 0,
  exclusion_count: () => 0,
  allow_playing: () => null,
  paging: (args) => {
    const slides = FILES.get(String(args.path))?.slides;
    return slides ? { pages: slides.length, note: null } : null;
  },
  page_painted: () => null,
  home_dir: () => HOME,
  survey_user_files: () => null,
  watch_shown_folder: () => null,
  list_dir: () => null,
  show_folder_previews: () => null,
  report_page_error: (args) => console.error("demo page error:", args.message),
  open_project_page: () => void window.open(GITHUB, "_blank", "noopener"),
};

export async function answer(command: string, args: Record<string, unknown>): Promise<unknown> {
  void loadIndex();
  const known = ANSWERS[command];
  if (known) return known(args);
  (window.__demoAsked ??= []).push({ command, args });
  return null;
}

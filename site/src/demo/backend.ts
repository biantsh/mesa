import type { CodePart, CodeView, MatchSnippet, SearchHit } from "../../../app/src/api";
import { isMac } from "../../../app/src/platform";
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
  about: string;
  at?: number;
  picture?: string;
  slides?: string[];
  text?: string[];
  source?: string;
  project?: string;
}

const LIBRARY: DemoFile[] = [
  { path: `${HOME}/Pictures/DSC_0187.jpg`, category: "image", date: [2024, 8, 14], size: 6_120_000, about: "dog beach sand sea running", at: 0, picture: "beach-dog-1.svg" },
  { path: `${HOME}/Pictures/DSC_0191.jpg`, category: "image", date: [2024, 8, 14], size: 5_870_000, about: "dog beach sunset sea", at: 0, picture: "beach-dog-2.svg" },
  { path: `${HOME}/Pictures/IMG_2231.jpg`, category: "image", date: [2024, 7, 2], size: 4_310_000, about: "beach sea waves holiday", at: 0, picture: "beach-sunset.svg" },
  { path: `${HOME}/Pictures/IMG_1984.jpg`, category: "image", date: [2024, 5, 19], size: 3_980_000, about: "dog park grass ball", at: 0, picture: "park-dog.svg" },
  { path: `${HOME}/Pictures/IMG_1702.jpg`, category: "image", date: [2024, 3, 9], size: 3_420_000, about: "garden flowers spring", at: 0, picture: "garden.svg" },
  { path: `${HOME}/Movies/Holiday day 2.mov`, category: "video", date: [2024, 8, 15], size: 412_000_000, about: "dog beach sea running holiday", at: 102_000, picture: "beach-video.svg" },
  { path: `${HOME}/Pictures/IMG_0412.jpg`, category: "image", date: [2025, 6, 1], size: 4_020_000, about: "dog beach sand", at: 0, picture: "beach-dog-3.svg" },
  { path: `${HOME}/Pictures/IMG_2240.jpg`, category: "image", date: [2024, 7, 3], size: 4_120_000, about: "beach sea waves surf", at: 0, picture: "waves.svg" },
  { path: `${HOME}/Pictures/IMG_2251.jpg`, category: "image", date: [2024, 7, 3], size: 3_870_000, about: "dog sea swimming", at: 0, picture: "dog-swim.svg" },
  { path: `${HOME}/Pictures/DSC_0203.jpg`, category: "image", date: [2024, 8, 12], size: 5_990_000, about: "dog beach ball sand", at: 0, picture: "dog-ball.svg" },
  { path: `${HOME}/Movies/Puppy.mov`, category: "video", date: [2023, 5, 21], size: 188_000_000, about: "dog running grass park", at: 14_000, picture: "park-video.svg" },
  { path: `${HOME}/Pictures/IMG_0877.jpg`, category: "image", date: [2023, 9, 10], size: 3_210_000, about: "dog sand dunes", at: 0, picture: "dunes-dog.svg" },
  { path: `${HOME}/Pictures/IMG_3310.heic`, category: "image", date: [2025, 8, 2], size: 2_640_000, about: "beach sunset sea", at: 0, picture: "sunset2.svg" },
  { path: `${HOME}/Pictures/IMG_3322.heic`, category: "image", date: [2025, 8, 2], size: 2_710_000, about: "dog beach running sea", at: 0, picture: "beach-dog-4.svg" },
  { path: `${HOME}/Movies/Coast walk.mp4`, category: "video", date: [2025, 4, 12], size: 264_000_000, about: "sea cliffs coast walk dog", at: 61_000, picture: "cliffs.svg" },
  { path: `${HOME}/Pictures/IMG_1105.jpg`, category: "image", date: [2023, 6, 18], size: 3_540_000, about: "lake dog swimming", at: 0, picture: "lake-dog.svg" },
  { path: `${HOME}/Pictures/DSC_0150.jpg`, category: "image", date: [2024, 8, 13], size: 5_430_000, about: "beach umbrella sand sea", at: 0, picture: "umbrella.svg" },
  { path: `${HOME}/Pictures/IMG_0901.jpg`, category: "image", date: [2023, 9, 11], size: 2_980_000, about: "sand castle beach", at: 0, picture: "sandcastle.svg" },
  { path: `${HOME}/Movies/Drone beach.mp4`, category: "video", date: [2024, 8, 17], size: 342_000_000, about: "beach sea aerial waves", at: 8_000, picture: "aerial.svg" },
  { path: `${HOME}/Pictures/IMG_4120.jpg`, category: "image", date: [2026, 10, 8], size: 3_412_000, about: "receipt tires car shop", at: 0, picture: "receipt-photo.svg" },
  { path: `${HOME}/Documents/Car expenses 2026.pdf`, category: "document", date: [2026, 11, 6], size: 184_000, about: "car expenses tires receipt insurance", at: 3, picture: "car-expenses.svg" },
  { path: `${HOME}/Desktop/Screenshot 2026-04-02.png`, category: "image", date: [2026, 4, 2], size: 512_000, about: "order tires receipt shop", at: 0, picture: "order-screenshot.svg" },
  {
    path: `${HOME}/Documents/Work/Q3 2026 Business Review.pptx`,
    category: "document",
    date: [2026, 10, 13],
    size: 8_640_000,
    about: "subscription churn coffee business review quarterly roasters",
    at: 4,
    picture: "q3-review/",
    slides: [
      "Larkspur Coffee Roasters. Q3 2026 Business Review. Leadership team, October 14, 2026",
      "Agenda. Q3 at a glance: revenue, subscribers and margin. Subscriptions: churn, pauses and delivery options. Wholesale & cafés: new partners by region. New roasts: what launched and how it sold. Q4 priorities: gift season, the app and a second roaster",
      "Q3 at a glance. Revenue $4.2M, up 18% vs Q3 2025. Active subscribers 31,400, up 2,900 this quarter. Gross margin 41.3%, up 1.8 points. Net Promoter Score 62, up 5 points. Our best quarter since we opened, and the first with every channel growing",
      "Subscription churn fell to 3.1%. Monthly churn: the share of active subscribers who cancelled. April 4.9%, May 4.6%, June 4.6%, July 4.0%, August 3.5%, September 3.1%. Changes shipped July 1. What changed: pause instead of cancel, 1 in 3 who start to cancel now pause. Flexible delivery every 2, 3 or 4 weeks, changed any time. Roaster’s choice box re-launched in July: 4,100 subscribers. Every point of churn we cut is worth about $310K a year in revenue",
      "Wholesale up 24%, led by 18 new cafés. New café partners by region: Portland 8, Seattle 5, Bay Area 3, Boise 2. “Our customers ask for the Guji by name now. It sold out in our shop before it did online.” Maya Ortiz, owner, Fernwood Café, Portland. 62 wholesale accounts. $1.1M wholesale revenue in Q3. 96% of cafés reordered",
      "Guji Highlands sold out in 9 days. Guji Highlands, Ethiopia, natural: blueberry, jasmine, cacao nib; 2,400 bags, sold out in 9 days. Night Shift, espresso blend: dark chocolate, molasses, orange; our best-selling espresso by week 3. Decaf Huila, Colombia, Swiss Water: toffee, red apple, brown sugar; now 9% of subscription boxes",
      "Q4 priorities. Holiday gift boxes: 12,000 boxes in three sizes, corporate orders open Nov 3, last shipping day Dec 12. Subscriber app: skip, swap and pause on the phone, beta with 500 subscribers in November, launch target December 1. Second roaster: 35 kg roaster in our Salem site, doubles capacity to 1.2 t a week, commissioned by November 20",
      "Thank you. Questions and discussion. Next review: January 13, 2027",
    ],
  },
  { path: `${HOME}/Documents/Work/Subscriber survey 2026.pdf`, category: "document", date: [2026, 9, 22], size: 1_240_000, about: "subscription survey subscribers coffee feedback", at: 2, picture: "survey.svg" },
  { path: `${HOME}/Desktop/Screenshot 2026-10-02.png`, category: "image", date: [2026, 10, 2], size: 688_000, about: "churn dashboard subscription chart", at: 0, picture: "churn-dashboard.svg" },
  {
    path: `${HOME}/Desktop/confusion_matrix.png`,
    category: "image",
    date: [2026, 11, 12],
    size: 41_300,
    about: "chart matplotlib confusion matrix iris classifier",
    at: 0,
    picture: "charts/confusion-matrix.svg",
    text: ["Confusion matrix: logistic regression", "setosa", "versicolor", "virginica", "Predicted label", "True label", "15 0 0", "0 14 1", "0 2 13"],
  },
  {
    path: `${HOME}/Projects/iris-classifier/evaluate.py`,
    category: "code",
    date: [2026, 11, 12],
    size: evaluatePy.length,
    about: "python matplotlib iris classifier confusion matrix learning curve",
    source: evaluatePy,
    project: "iris-classifier",
  },
  { path: `${HOME}/Projects/iris-classifier/figures/confusion_matrix.png`, category: "image", date: [2026, 11, 12], size: 41_300, about: "chart matplotlib confusion matrix iris classifier", at: 0, picture: "charts/confusion-matrix.svg" },
  { path: `${HOME}/Projects/iris-classifier/figures/learning_curve.png`, category: "image", date: [2026, 11, 12], size: 52_800, about: "chart matplotlib learning curve iris classifier", at: 0, picture: "charts/learning-curve.svg" },
];

const LITTLE = new Set(["a", "an", "the", "of", "my", "at", "on", "in", "for", "to", "new", "with", "and", "from"]);

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

function search(text: string, filter: { category?: Category | null; from?: number | null; to?: number | null }): SearchHit[] {
  const words = text.toLowerCase().split(/\s+/).filter((word) => word && !LITTLE.has(word));
  const kept = LIBRARY.filter(
    (file) =>
      (!filter.category || file.category === filter.category) &&
      (filter.from == null || modified(file) >= filter.from) &&
      (filter.to == null || modified(file) < filter.to),
  );
  const ranked = kept
    .map((file) => {
      const about = file.about.split(" ");
      const matches = words.filter((word) => about.some((one) => one.startsWith(word))).length;
      return { file, matches, slide: slideWith(file, text) };
    })
    .filter(({ matches, slide }) => words.length === 0 || matches > 0 || slide !== null)
    .sort((a, b) => Number(b.slide !== null) - Number(a.slide !== null) || b.matches - a.matches || modified(b.file) - modified(a.file));
  return ranked.map(({ file, matches, slide }, i) =>
    hitOf(file, {
      score: words.length ? 0.4 + (0.3 * matches) / words.length - i * 0.01 : 0,
      reason: slide !== null ? "words" : words.length ? "meaning" : null,
      byWords: slide !== null ? "phrase" : null,
      at: slide ?? (words.length ? (file.at ?? null) : null),
    }),
  );
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

const CLOSE_OF_SIMILAR = 12;
const rest = new Map<number, SearchHit[]>();

function similar(examples: string[], filter: Parameters<typeof search>[1]): SearchHit[] {
  const files = LIBRARY.filter((file) => examples.includes(file.path));
  const clued = codeWithClues(cluesOf(files), (file) => !filter.category || file.category === filter.category);
  const words = new Set(files.flatMap((file) => file.about.split(" ")));
  const alike = search([...words].join(" "), filter).filter((hit) => !examples.includes(hit.path) && !clued.some((one) => one.path === hit.path));
  return [...clued, ...alike];
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

const ANSWERS: Record<string, (args: Record<string, unknown>) => unknown> = {
  model_status: () => ({ state: "ready" }),
  index_view: () => ({
    overview: { empty: false, progress: null, everythingIndexed: true, ...TOTALS },
    folder: null,
    lists: null,
  }),
  get_settings: () => ({ gpuAcceleration: true, alwaysFullSpeed: false, startAtSignIn: false }),
  get_search_hotkey: () => ({
    active: isMac ? { keys: ["⌥", "Space"], name: "Option+Space" } : { keys: ["Alt", "Space"], name: "Alt+Space" },
    chosenIn: { kind: "mesa" },
  }),
  get_indexed_types: () => ["document", "image", "video", "audio", "code"],
  embedding_gpu: () => ({ name: "Apple M2", shared: true }),
  search: (args) => {
    const id = ++searches;
    const filter = (args.filter ?? {}) as Parameters<typeof search>[1];
    if (Array.isArray(args.files) && args.files.length > 0) {
      const found = similar(args.files as string[], filter);
      rest.clear();
      rest.set(id, found.slice(CLOSE_OF_SIMILAR));
      return { id, hits: found.slice(0, CLOSE_OF_SIMILAR), more: found.length > CLOSE_OF_SIMILAR, close: true, moreClose: false };
    }
    return { id, hits: search(String(args.query ?? ""), filter), more: false, close: true, moreClose: false };
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
  const known = ANSWERS[command];
  if (known) return known(args);
  (window.__demoAsked ??= []).push({ command, args });
  return null;
}

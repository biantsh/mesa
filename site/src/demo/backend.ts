import type { SearchHit } from "../../../app/src/api";
import { isMac } from "../../../app/src/platform";
import { GITHUB } from "../links";

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
];

const LITTLE = new Set(["a", "an", "the", "of", "my", "at", "on", "in", "for", "to", "new", "with", "and", "from"]);

function modified(file: DemoFile): number {
  const [year, month, day] = file.date;
  return new Date(year, month - 1, day, 12).getTime();
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
      return { file, matches };
    })
    .filter(({ matches }) => words.length === 0 || matches > 0)
    .sort((a, b) => b.matches - a.matches || modified(b.file) - modified(a.file));
  return ranked.map(({ file, matches }, i) => {
    const parts = file.path.split("/");
    return {
      path: file.path,
      name: parts[parts.length - 1],
      folder: parts[parts.length - 2],
      category: file.category,
      modified: modified(file),
      size: file.size,
      score: words.length ? 0.4 + (0.3 * matches) / words.length - i * 0.01 : 0,
      reason: words.length ? "meaning" : null,
      byWords: null,
      at: words.length ? (file.at ?? null) : null,
      copies: [],
    };
  });
}

const CLOSE_OF_SIMILAR = 12;
const rest = new Map<number, SearchHit[]>();

function similar(examples: string[], filter: Parameters<typeof search>[1]): SearchHit[] {
  const words = new Set(LIBRARY.filter((file) => examples.includes(file.path)).flatMap((file) => file.about.split(" ")));
  return search([...words].join(" "), filter).filter((hit) => !examples.includes(hit.path));
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

const ANSWERS: Record<string, (args: Record<string, unknown>) => unknown> = {
  model_status: () => ({ state: "ready" }),
  index_view: () => ({
    overview: { empty: false, progress: null, everythingIndexed: true, files: 5_887, bytes: Math.round(60.4 * 1_048_576) },
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
  match_details: () => ({ snippets: [], closest: null, code: null, alsoIn: [] }),
  code_projects: () => [],
  also_in: () => [],
  code_peek: () => [],
  prepare_example: () => null,
  example_progress: (args) => (Array.isArray(args.paths) ? args.paths.map(() => ({ made: 1, of: 1, reading: false })) : []),
  count_with_data: () => 0,
  count_indexed_of_type: () => 0,
  exclusion_count: () => 0,
  allow_playing: () => null,
  paging: () => null,
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

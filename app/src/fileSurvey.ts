import type { Category, Estimate, Survey } from "./api";

const number = new Intl.NumberFormat();

export function filesOf(survey: Survey, types: Category[]): number {
  return types.reduce((sum, type) => sum + (survey.counts[type] ?? 0), 0);
}

export function describeTotal(survey: Survey, types: Category[]): string {
  if (types.length === 0) return "";
  const files = filesOf(survey, types);
  const more = survey.notAsked.length > 0;
  return `${number.format(files)}${more ? "+" : ""} ${files === 1 && !more ? "file" : "files"}`;
}

export function estimateSeconds(estimate: Estimate, types: Category[]): number | null {
  let total = 0;
  for (const type of types) {
    const seconds = estimate.seconds[type];
    if (seconds === null) return null;
    total += seconds;
  }
  return total;
}

const FIVES_FROM_MINUTES = 10;
const WHOLE_HOURS_FROM = 10;
const DAYS_FROM_HOURS = 48;
const HALVES = 2;

const SECONDS = { minute: 60, hour: 3600, day: 86_400 };

export function formatEstimateRange(fast: number, gentle: number, more = false): string {
  if (gentle < SECONDS.minute) return more ? "a minute or more" : "under a minute";
  const plus = more ? "+" : "";
  const say = (low: number, high: number, unit: keyof typeof SECONDS) => {
    const lowest = Math.min(low, high);
    const word = high === 1 && lowest === 1 ? unit : `${unit}s`;
    return lowest === high ? `about ${high}${plus} ${word}` : `${lowest}-${high}${plus} ${word}`;
  };
  const minutes = gentle / SECONDS.minute;
  const highMinutes = minutes < FIVES_FROM_MINUTES ? Math.round(minutes) : Math.round(minutes / 5) * 5;
  if (highMinutes < 60) return say(Math.max(1, Math.round(fast / SECONDS.minute)), highMinutes, "minute");
  const half = (seconds: number, unit: number) => Math.max(1 / HALVES, Math.round((seconds / unit) * HALVES) / HALVES);
  const hours = gentle / SECONDS.hour;
  if (hours < DAYS_FROM_HOURS) {
    const high = hours < WHOLE_HOURS_FROM ? Math.max(1, Math.round(hours * HALVES) / HALVES) : Math.round(hours);
    return say(half(fast, SECONDS.hour), high, "hour");
  }
  return say(half(fast, SECONDS.day), Math.round(gentle / SECONDS.day), "day");
}

export function joinNames(names: string[]): string {
  if (names.length <= 1) return names.join("");
  return `${names.slice(0, -1).join(", ")} and ${names[names.length - 1]}`;
}

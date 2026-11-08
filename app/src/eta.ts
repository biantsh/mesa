export interface Sample {
  t: number;
  done: number;
}

export const WINDOW_MS = 30_000;
const MIN_SPAN_MS = 4_000;

export function addSample(samples: Sample[], sample: Sample): Sample[] {
  const last = samples[samples.length - 1];
  const kept = last && sample.done < last.done ? [] : samples.slice();
  kept.push(sample);
  const cutoff = sample.t - WINDOW_MS;
  while (kept.length > 2 && kept[1].t < cutoff) kept.shift();
  return kept;
}

export function speedOf(samples: Sample[]): number | null {
  if (samples.length < 2) return null;
  const first = samples[0];
  const last = samples[samples.length - 1];
  const span = last.t - first.t;
  const files = last.done - first.done;
  if (span < MIN_SPAN_MS || files <= 0) return null;
  return files / (span / 1000);
}

export function formatEta(seconds: number | null): string | null {
  if (seconds === null || !Number.isFinite(seconds)) return null;
  if (seconds < 45) return "< 1 min left";
  const minutes = Math.round(seconds / 60);
  if (minutes < 60) return `${minutes} min left`;
  const hours = Math.floor(minutes / 60);
  const rest = minutes % 60;
  return rest === 0 ? `${hours} h left` : `${hours} h ${rest} min left`;
}

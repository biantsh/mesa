export type NavKey = "ArrowLeft" | "ArrowRight" | "ArrowUp" | "ArrowDown" | "Home" | "End";

export const NAV_KEYS: ReadonlySet<string> = new Set<NavKey>([
  "ArrowLeft",
  "ArrowRight",
  "ArrowUp",
  "ArrowDown",
  "Home",
  "End",
]);

export function navigate(selectable: readonly boolean[], from: number | null, key: NavKey, columns: number): number | null {
  const last = selectable.length - 1;
  const first = selectable.indexOf(true);
  if (first < 0) return null;
  const end = selectable.lastIndexOf(true);
  if (key === "Home") return first;
  if (key === "End") return end;
  const forward = key === "ArrowRight" || key === "ArrowDown";
  if (from === null) return forward ? first : end;

  const step = key === "ArrowLeft" || key === "ArrowRight" ? 1 : Math.max(1, columns);
  const delta = forward ? step : -step;
  for (let at = from + delta; at >= 0 && at <= last; at += delta) {
    if (selectable[at]) return at;
  }
  if (step > 1 && forward && Math.floor(from / step) < Math.floor(last / step)) return end > from ? end : null;
  return null;
}

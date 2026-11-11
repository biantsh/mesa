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
  return navigateAmong(selectable.length, (at) => selectable[at], from, key, columns);
}

export function navigateAmong(count: number, can: (at: number) => boolean, from: number | null, key: NavKey, columns: number): number | null {
  const last = count - 1;
  const first = () => {
    for (let at = 0; at <= last; at++) if (can(at)) return at;
    return -1;
  };
  const end = () => {
    for (let at = last; at >= 0; at--) if (can(at)) return at;
    return -1;
  };
  if (key === "Home" || key === "End" || from === null) {
    const [start, finish] = [first(), end()];
    if (start < 0) return null;
    if (key === "Home") return start;
    if (key === "End") return finish;
    const forward = key === "ArrowRight" || key === "ArrowDown";
    return forward ? start : finish;
  }
  const forward = key === "ArrowRight" || key === "ArrowDown";
  const step = key === "ArrowLeft" || key === "ArrowRight" ? 1 : Math.max(1, columns);
  const delta = forward ? step : -step;
  for (let at = from + delta; at >= 0 && at <= last; at += delta) {
    if (can(at)) return at;
  }
  if (step > 1 && forward && Math.floor(from / step) < Math.floor(last / step)) {
    const finish = end();
    return finish > from ? finish : null;
  }
  return null;
}

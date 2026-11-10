const ATTRIBUTE = "data-resizing";
const SETTLE_MS = 150;
const holds = new Map<Element, number>();

export const isResizing = () => holds.size > 0;

export function holdStill(within: Element = document.documentElement): () => void {
  const count = holds.get(within) ?? 0;
  holds.set(within, count + 1);
  if (count === 0) within.setAttribute(ATTRIBUTE, "");
  let held = true;
  return () => {
    if (!held) return;
    held = false;
    const left = (holds.get(within) ?? 1) - 1;
    if (left > 0) {
      holds.set(within, left);
    } else {
      holds.delete(within);
      within.removeAttribute(ATTRIBUTE);
    }
  };
}

export function jumpWithTheWindow() {
  let release: (() => void) | null = null;
  let timer = 0;
  window.addEventListener("resize", () => {
    release ??= holdStill();
    window.clearTimeout(timer);
    timer = window.setTimeout(() => {
      release?.();
      release = null;
    }, SETTLE_MS);
  });
}

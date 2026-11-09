const ATTRIBUTE = "data-resizing";
const SETTLE_MS = 150;
let holds = 0;

export function holdStill(): () => void {
  const root = document.documentElement;
  if (holds++ === 0) root.setAttribute(ATTRIBUTE, "");
  let held = true;
  return () => {
    if (!held) return;
    held = false;
    if (--holds === 0) root.removeAttribute(ATTRIBUTE);
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

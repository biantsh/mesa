import { platform } from "./platform";

let pointer = false;

if (typeof window !== "undefined") {
  window.addEventListener("pointerdown", () => (pointer = true), { capture: true, passive: true });
  window.addEventListener("keydown", () => (pointer = false), { capture: true, passive: true });
}

export function skipGivingBackTo(element: Element): boolean {
  return platform === "linux" && pointer && element instanceof HTMLButtonElement;
}

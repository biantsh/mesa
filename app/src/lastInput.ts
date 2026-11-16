// Tracks whether the last input was a pointer press, so focus restored after a click doesn't show a ring
// (WebKit, and WebView2 after keyboard use).
import { platform } from "./platform";

let pointer = false;

if (typeof window !== "undefined") {
  window.addEventListener("pointerdown", () => (pointer = true), { capture: true, passive: true });
  window.addEventListener("keydown", () => (pointer = false), { capture: true, passive: true });
}

export function lastWasPointer(): boolean {
  return pointer;
}

function clickFocuses(element: Element): boolean {
  if (element instanceof HTMLButtonElement) return platform !== "mac";
  return element.getAttribute("role") === "button";
}

export function skipGivingBackTo(element: Element, openedByPointer: boolean): boolean {
  if (!clickFocuses(element)) return false;
  return openedByPointer || (pointer && platform !== "windows");
}

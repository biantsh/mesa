import { currentWindow } from "./window";

const ATTRIBUTE = "data-pointer-unknown";

export function guardAgainstStaleHover() {
  const root = document.documentElement;
  const known = () => root.removeAttribute(ATTRIBUTE);
  const unknown = () => {
    root.setAttribute(ATTRIBUTE, "");
    window.addEventListener("pointermove", known, { once: true, capture: true });
  };

  window.addEventListener("blur", unknown);
  document.addEventListener("visibilitychange", unknown);
  void currentWindow()
    ?.onFocusChanged(({ payload: focused }) => !focused && unknown())
    .catch(() => {});
}

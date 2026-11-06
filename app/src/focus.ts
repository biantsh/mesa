export function releaseFocus() {
  const focused = document.activeElement;
  if (focused instanceof HTMLElement) focused.closest<HTMLElement>("[tabindex='-1']")?.focus({ preventScroll: true });
}

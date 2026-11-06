export const BACKGROUND_PRESS = "mesa:background-press";

export function pressBackground() {
  window.dispatchEvent(new Event(BACKGROUND_PRESS));
}

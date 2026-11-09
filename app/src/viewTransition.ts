import { flushSync } from "react-dom";
import { prefersReducedMotion } from "./motion";

type Start = (update: () => void) => { finished: Promise<unknown> };

export function changeWithTransition(change: () => void, { onlyMorph = false } = {}) {
  const start = (document as Document & { startViewTransition?: Start }).startViewTransition;
  if (!start || prefersReducedMotion()) {
    change();
    return;
  }
  const root = document.documentElement;
  if (onlyMorph) root.dataset.morphOnly = "";
  const transition = start.call(document, () => flushSync(change));
  const over = () => delete root.dataset.morphOnly;
  transition.finished.then(over, over);
}

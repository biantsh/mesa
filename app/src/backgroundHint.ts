import { onceFlag } from "./onceFlag";

export const HINT_DELAY_MS = 18_000;
const flag = onceFlag("mesa.backgroundHintSeen");
export const hasShownBackgroundHint = flag.seen;
export const markBackgroundHintShown = flag.mark;
export const forgetBackgroundHintForTests = flag.forgetForTests;

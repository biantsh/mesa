import { onceFlag } from "./onceFlag";
import { tipsAreOnce } from "./tipsOnce";

export const SHORTCUT_TIP_DELAY_MS = 2_000;
export const SHORTCUT_TIP_COUNTS_AFTER_MS = 3_000;
export const SHORTCUT_TIP_VISIBLE_MS = 12_000;

export const shortcutTipFlag = onceFlag("mesa.shortcutTipSeen");
export const hasSeenShortcutTip = shortcutTipFlag.seen;
export const markShortcutTipSeen = shortcutTipFlag.mark;

export function markShortcutTipKnown(): void {
  if (tipsAreOnce()) markShortcutTipSeen();
}

export const forgetShortcutTipForTests = shortcutTipFlag.forgetForTests;

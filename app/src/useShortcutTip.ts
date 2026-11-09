import { useEffect, useState } from "react";
import { api } from "./api";
import {
  SHORTCUT_TIP_COUNTS_AFTER_MS,
  SHORTCUT_TIP_DELAY_MS,
  SHORTCUT_TIP_VISIBLE_MS,
  shortcutTipFlag,
} from "./shortcutTip";
import { useOnceTip } from "./useOnceTip";

interface Options {
  onScreen: boolean;
  worth: boolean;
}

export function useShortcutTip({ onScreen, worth }: Options): { visible: boolean; keys: string[] } {
  const [keys, setKeys] = useState<string[] | null>(null);
  const { visible, seen, here } = useOnceTip({
    flag: shortcutTipFlag,
    owner: "shortcut",
    onScreen,
    ready: worth && keys !== null && keys.length > 0,
    endsWhenNotReady: true,
    delayMs: SHORTCUT_TIP_DELAY_MS,
    countsAfterMs: SHORTCUT_TIP_COUNTS_AFTER_MS,
    visibleMs: SHORTCUT_TIP_VISIBLE_MS,
  });

  useEffect(() => {
    if (seen || keys !== null || !here || !worth) return;
    let current = true;
    api
      .getSearchHotkey()
      .then((status) => current && setKeys(status.active?.keys ?? []))
      .catch(() => current && setKeys([]));
    return () => {
      current = false;
    };
  }, [seen, keys, here, worth]);

  return { visible, keys: keys ?? [] };
}

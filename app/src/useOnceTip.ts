import { useCallback, useEffect, useState } from "react";
import type { OnceFlag } from "./onceFlag";
import { claimTipSlot } from "./tipSlot";
import { usePageVisible } from "./usePageVisible";

interface Options {
  flag: OnceFlag;
  owner: string;
  onScreen: boolean;
  ready: boolean;
  endsWhenNotReady?: boolean;
  keptWhile?: boolean;
  delayMs: number;
  countsAfterMs: number;
  visibleMs: number;
}

export function useOnceTip({
  flag,
  owner,
  onScreen,
  ready,
  endsWhenNotReady = false,
  keptWhile = ready,
  delayMs,
  countsAfterMs,
  visibleMs,
}: Options) {
  const [seen, setSeen] = useState(flag.seen);
  const [visible, setVisible] = useState(false);
  const pageVisible = usePageVisible();
  const here = onScreen && pageVisible;

  const finish = useCallback(() => {
    flag.mark();
    setSeen(true);
    setVisible(false);
  }, [flag]);

  useEffect(() => {
    if (!seen && flag.seen()) setSeen(true);
  });

  useEffect(() => {
    if (seen || visible || !here || !ready) return;
    const show = () => claimTipSlot(owner) && setVisible(true);
    let timer = window.setTimeout(show, delayMs);
    const restart = () => {
      window.clearTimeout(timer);
      timer = window.setTimeout(show, delayMs);
    };
    window.addEventListener("keydown", restart, { capture: true });
    window.addEventListener("pointerdown", restart, { capture: true });
    return () => {
      window.clearTimeout(timer);
      window.removeEventListener("keydown", restart, { capture: true });
      window.removeEventListener("pointerdown", restart, { capture: true });
    };
  }, [seen, visible, here, ready, owner, delayMs]);

  const stays = here && (keptWhile || !endsWhenNotReady);
  useEffect(() => {
    if (visible && !stays) setVisible(false);
  }, [visible, stays]);

  useEffect(() => {
    if (!visible) return;
    const counted = window.setTimeout(() => {
      flag.mark();
      setSeen(true);
    }, countsAfterMs);
    const over = window.setTimeout(() => setVisible(false), visibleMs);
    const press = () => finish();
    const key = (e: KeyboardEvent) => {
      if (e.key === "Escape") finish();
    };
    window.addEventListener("pointerdown", press, { capture: true });
    window.addEventListener("keydown", key, { capture: true });
    return () => {
      window.clearTimeout(counted);
      window.clearTimeout(over);
      window.removeEventListener("pointerdown", press, { capture: true });
      window.removeEventListener("keydown", key, { capture: true });
    };
  }, [visible, flag, finish, countsAfterMs, visibleMs]);

  return { visible, seen, here, finish };
}

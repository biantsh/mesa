import { useEffect } from "react";
import { TIP_COUNTS_AFTER_MS, TIP_DELAY_MS, TIP_VISIBLE_MS, speedTipFlag } from "./speedTip";
import { useOnceTip } from "./useOnceTip";

interface Options {
  onScreen: boolean;
  worth: boolean;
  turbo: boolean;
}

export function useSpeedTip({ onScreen, worth, turbo }: Options): boolean {
  const { visible, seen, finish } = useOnceTip({
    flag: speedTipFlag,
    owner: "speed",
    onScreen: onScreen && !turbo,
    ready: worth,
    delayMs: TIP_DELAY_MS,
    countsAfterMs: TIP_COUNTS_AFTER_MS,
    visibleMs: TIP_VISIBLE_MS,
  });
  useEffect(() => {
    if (turbo && !seen) finish();
  }, [turbo, seen, finish]);
  return visible;
}

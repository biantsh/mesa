import { onceFlag } from "./onceFlag";

export const TIP_MIN_SECONDS_LEFT = 5 * 60;
export const TIP_DELAY_MS = 4_000;
export const TIP_COUNTS_AFTER_MS = 3_000;
export const TIP_VISIBLE_MS = 10_000;

export const speedTipFlag = onceFlag("mesa.speedTipSeen");
export const hasSeenSpeedTip = speedTipFlag.seen;
export const markSpeedTipSeen = speedTipFlag.mark;

export interface TipSituation {
  secondsLeft: number | null;
  paused: boolean;
  counting: boolean;
  turbo: boolean;
}

export function worthATip({ secondsLeft, paused, counting, turbo }: TipSituation): boolean {
  return secondsLeft !== null && secondsLeft >= TIP_MIN_SECONDS_LEFT && !paused && !counting && !turbo;
}

export const forgetSpeedTipForTests = speedTipFlag.forgetForTests;

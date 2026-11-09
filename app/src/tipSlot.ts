import { tipsAreOnce } from "./tipsOnce";

let holder: string | null = null;

export function claimTipSlot(owner: string): boolean {
  if (!tipsAreOnce()) return true;
  if (holder === null || holder === owner) {
    holder = owner;
    return true;
  }
  return false;
}

export function freeTipSlotForTests(): void {
  holder = null;
}

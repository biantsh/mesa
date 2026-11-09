import { tipsAreOnce } from "./tipsOnce";

export interface OnceFlag {
  seen: () => boolean;
  mark: () => void;
  forgetForTests: () => void;
}

export function onceFlag(key: string): OnceFlag {
  let thisRun = false;
  return {
    seen() {
      if (thisRun) return true;
      if (!tipsAreOnce()) return false;
      try {
        return localStorage.getItem(key) === "1";
      } catch {
        return true;
      }
    },
    mark() {
      thisRun = true;
      if (!tipsAreOnce()) return;
      try {
        localStorage.setItem(key, "1");
      } catch {
      }
    },
    forgetForTests() {
      thisRun = false;
    },
  };
}

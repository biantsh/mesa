let once = !import.meta.env.DEV;

export function tipsAreOnce(): boolean {
  return once;
}

export function setTipsAreOnceForTests(value: boolean): void {
  once = value;
}

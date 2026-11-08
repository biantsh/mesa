export function fitQuoted(text: string, width: number, measure: (text: string) => number): string {
  const whole = `\u201c${text}\u201d`;
  if (width <= 0 || measure(whole) <= width) return whole;
  const letters = Array.from(text);
  const cut = (count: number) => `\u201c${letters.slice(0, count).join("").trimEnd()}\u2026\u201d`;
  let low = 0;
  let high = letters.length;
  while (low < high) {
    const middle = Math.ceil((low + high) / 2);
    if (measure(cut(middle)) <= width) low = middle;
    else high = middle - 1;
  }
  return cut(low);
}

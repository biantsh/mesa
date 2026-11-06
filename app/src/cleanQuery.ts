const INVISIBLE = /[\u00AD\u200B-\u200F\u2060\uFEFF]/g;
const SPACES = /[\s\p{Zs}]+/gu;
const CONTROL = /\p{Cc}/gu;
const APOSTROPHES = /[\u2018\u2019\u02BC]/g;

const LETTERS = /\p{L}+/gu;

const UPPER = /\p{Lu}/u;
const LOWER = /\p{Ll}/u;

const HUMPS = /^\p{Lu}?\p{Ll}+(?:\p{Lu}\p{Ll}+)+$/u;
const PLURAL_ABBREVIATION = /^\p{Lu}{2,}s$/u;
const SYSTEM_NAME = /^(?:i|mac|tv|watch|ipad|vision)OS$/u;

function fixCase(letters: string): string {
  if (!UPPER.test(letters) || !LOWER.test(letters)) return letters;
  const first = letters.charAt(0);
  const rest = letters.slice(first.length);
  if (UPPER.test(first) && !UPPER.test(rest)) return letters;
  if (HUMPS.test(letters) || PLURAL_ABBREVIATION.test(letters) || SYSTEM_NAME.test(letters)) return letters;
  const lower = letters.toLowerCase();
  return UPPER.test(first) ? first + lower.slice(first.toLowerCase().length) : lower;
}

export function cleanQuery(text: string): string {
  return text
    .normalize("NFC")
    .replace(INVISIBLE, "")
    .replace(SPACES, " ")
    .replace(CONTROL, "")
    .trim()
    .replace(APOSTROPHES, "'")
    .replace(LETTERS, fixCase);
}

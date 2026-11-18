import type { CodeKind, CodePart } from "../../../app/src/api";

const KEYWORDS = new Set([
  "and", "as", "assert", "async", "await", "break", "class", "continue", "def", "del", "elif", "else", "except", "finally", "for", "from",
  "global", "if", "import", "in", "is", "lambda", "nonlocal", "not", "or", "pass", "raise", "return", "try", "while", "with", "yield",
]);
const LITERALS = new Set(["True", "False", "None"]);
const SELVES = new Set(["self", "cls"]);
const TYPES = new Set(["int", "float", "str", "bool", "bytes", "list", "dict", "set", "tuple", "frozenset", "object"]);

const TOKEN = /([rRbBuUfF]{0,2})("""|'''|"|')|(#.*)|(\b\d[\d_]*(?:\.\d*)?(?:[eE][+-]?\d+)?\b)|([A-Za-z_]\w*)|(\s+)|(.)/gy;

function kindOf(word: string, previousWord: string | null, called: boolean, afterDot: boolean): CodeKind | undefined {
  if (LITERALS.has(word)) return "number";
  if (SELVES.has(word)) return "variable";
  if (TYPES.has(word) && !called) return "type";
  if (KEYWORDS.has(word)) return "keyword";
  if (previousWord === "def") return "function";
  if (previousWord === "class") return "type";
  if (called) return "function";
  if (afterDot) return "property";
  if (word.length > 1 && /[A-Z]/.test(word) && !/[a-z]/.test(word)) return "number";
  if (/^[A-Z]/.test(word)) return "type";
  return undefined;
}

export function highlightPython(source: string): CodePart[][] {
  const lines: CodePart[][] = [];
  let open: string | null = null;
  let previousWord: string | null = null;
  for (const line of source.replace(/\r\n?/g, "\n").split("\n")) {
    const parts: CodePart[] = [];
    const add = (text: string, kind?: CodeKind) => {
      const last = parts[parts.length - 1];
      if (last && last.kind === kind) last.text += text;
      else parts.push(kind ? { text, kind } : { text });
    };
    let at = 0;
    if (open) {
      const end = line.indexOf(open);
      if (end < 0) {
        add(line, "string");
        lines.push(parts.length ? parts : [{ text: "" }]);
        continue;
      }
      add(line.slice(0, end + open.length), "string");
      at = end + open.length;
      open = null;
    }
    TOKEN.lastIndex = at;
    for (let found = TOKEN.exec(line); found; found = TOKEN.exec(line)) {
      const [whole, prefix, quote, comment, number, word] = found;
      if (quote) {
        const from = TOKEN.lastIndex;
        let end = from;
        for (; end < line.length; end++) {
          if (line[end] === "\\") end++;
          else if (line.startsWith(quote, end)) break;
        }
        if (end >= line.length) {
          add(prefix + quote + line.slice(from), "string");
          if (quote.length === 3) open = quote;
          TOKEN.lastIndex = line.length;
        } else {
          add(prefix + quote + line.slice(from, end + quote.length), "string");
          TOKEN.lastIndex = end + quote.length;
        }
        previousWord = null;
      } else if (comment) {
        add(comment, "comment");
      } else if (number) {
        add(number, "number");
        previousWord = null;
      } else if (word) {
        const called = /^\s*\(/.test(line.slice(TOKEN.lastIndex));
        const afterDot = line.slice(0, found.index).trimEnd().endsWith(".");
        add(word, kindOf(word, previousWord, called, afterDot));
        previousWord = word;
      } else {
        add(whole);
        if (!/^\s+$/.test(whole)) previousWord = null;
      }
    }
    lines.push(parts.length ? parts : [{ text: "" }]);
  }
  return lines;
}

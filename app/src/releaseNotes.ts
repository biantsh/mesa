export type Inline = { kind: "text" | "bold" | "italic" | "code"; text: string };

export type ListItem = { inlines: Inline[]; items: ListItem[] };

export type Block =
  | { kind: "heading"; level: number; inlines: Inline[] }
  | { kind: "paragraph"; inlines: Inline[] }
  | { kind: "list"; items: ListItem[] };

const HEADING = /^(#{1,6})\s+(.*)$/;
const ITEM = /^(\s*)[-*+]\s+(.*)$/;

type RawItem = { text: string; items: RawItem[] };
type Level = { indent: number; items: RawItem[] };

export function readNotes(notes: string): Block[] {
  const blocks: Block[] = [];
  let paragraph: string[] = [];
  let levels: Level[] = [];
  const endParagraph = () => {
    if (paragraph.length) blocks.push({ kind: "paragraph", inlines: readInline(paragraph.join(" ")) });
    paragraph = [];
  };
  const endList = () => {
    if (levels.length) blocks.push({ kind: "list", items: levels[0].items.map(finishItem) });
    levels = [];
  };
  const addItem = (indent: number, text: string) => {
    if (!levels.length) levels.push({ indent, items: [] });
    let top = levels[levels.length - 1];
    const parent = top.items[top.items.length - 1];
    if (indent > top.indent && parent) {
      top = { indent, items: parent.items };
      levels.push(top);
    } else {
      while (levels.length > 1 && indent < top.indent) {
        levels.pop();
        top = levels[levels.length - 1];
      }
    }
    top.items.push({ text, items: [] });
  };
  for (const raw of notes.split(/\r?\n/)) {
    const line = raw.trimEnd();
    const heading = HEADING.exec(line);
    const item = ITEM.exec(line);
    if (!line.trim()) {
      endParagraph();
    } else if (heading) {
      endParagraph();
      endList();
      blocks.push({ kind: "heading", level: heading[1].length, inlines: readInline(heading[2]) });
    } else if (item) {
      endParagraph();
      addItem(item[1].length, item[2]);
    } else if (levels.length && /^\s+/.test(raw)) {
      const top = levels[levels.length - 1];
      top.items[top.items.length - 1].text += ` ${line.trim()}`;
    } else {
      endList();
      paragraph.push(line.trim());
    }
  }
  endParagraph();
  endList();
  return blocks;
}

function finishItem(item: RawItem): ListItem {
  return { inlines: readInline(item.text), items: item.items.map(finishItem) };
}

// No lookbehind: WebKit before Safari 16.4 (macOS 13.0 to 13.2) can't parse it, and the page would fail to load.
const INLINE = /`([^`]+)`|\*\*(.+?)\*\*|__(.+?)__|\*(?!\s)(.+?)\*|\[([^\]]+)\]\([^)]*\)/g;

export function readInline(text: string): Inline[] {
  const out: Inline[] = [];
  const push = (kind: Inline["kind"], piece: string) => {
    if (!piece) return;
    const last = out[out.length - 1];
    if (kind === "text" && last?.kind === "text") last.text += piece;
    else out.push({ kind, text: piece });
  };
  let at = 0;
  for (const match of text.matchAll(INLINE)) {
    push("text", text.slice(at, match.index));
    const [, code, bold, bold2, italic, link] = match;
    if (code !== undefined) push("code", code);
    else if (bold !== undefined || bold2 !== undefined) push("bold", bold ?? bold2);
    else if (italic !== undefined) push("italic", italic);
    else push("text", link);
    at = match.index + match[0].length;
  }
  push("text", text.slice(at));
  return out;
}

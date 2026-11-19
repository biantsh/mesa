export type Inline = { kind: "text" | "bold" | "italic" | "code"; text: string };

export type Block =
  | { kind: "heading"; inlines: Inline[] }
  | { kind: "paragraph"; inlines: Inline[] }
  | { kind: "list"; items: Inline[][] };

const HEADING = /^#{1,6}\s+(.*)$/;
const ITEM = /^\s*[-*+]\s+(.*)$/;

export function readNotes(notes: string): Block[] {
  const blocks: Block[] = [];
  let paragraph: string[] = [];
  let list: string[] | null = null;
  const endParagraph = () => {
    if (paragraph.length) blocks.push({ kind: "paragraph", inlines: readInline(paragraph.join(" ")) });
    paragraph = [];
  };
  const endList = () => {
    if (list?.length) blocks.push({ kind: "list", items: list.map(readInline) });
    list = null;
  };
  for (const raw of notes.split(/\r?\n/)) {
    const line = raw.trimEnd();
    const heading = HEADING.exec(line);
    const item = ITEM.exec(line);
    if (!line.trim()) {
      endParagraph();
      endList();
    } else if (heading) {
      endParagraph();
      endList();
      blocks.push({ kind: "heading", inlines: readInline(heading[1]) });
    } else if (item) {
      endParagraph();
      (list ??= []).push(item[1]);
    } else if (list?.length && /^\s+/.test(raw)) {
      list[list.length - 1] += ` ${line.trim()}`;
    } else {
      endList();
      paragraph.push(line.trim());
    }
  }
  endParagraph();
  endList();
  return blocks;
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

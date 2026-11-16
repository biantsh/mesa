export type Paging = "page" | "slide";

export function pagingOf(name: string): Paging | null {
  const lower = name.toLowerCase();
  if (lower.endsWith(".pdf")) return "page";
  if ([".pptx", ".pptm", ".ppsx"].some((extension) => lower.endsWith(extension))) return "slide";
  return null;
}

export function pagingWord(paging: Paging): string {
  return paging === "page" ? "Page" : "Slide";
}

export function pageFromText(text: string, pages: number | null): number | null {
  if (!/^\d{1,6}$/.test(text.trim())) return null;
  const page = Number(text.trim());
  return page >= 1 && (pages === null || page <= pages) ? page : null;
}

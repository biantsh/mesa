export type Paging = "page" | "slide";

const PRESENTATIONS = [".pptx", ".pptm", ".ppsx", ".potx", ".potm", ".odp", ".otp", ".key"];
const LAID_OUT = [".docx", ".docm", ".dotx", ".dotm", ".xlsx", ".xlsm", ".xltx", ".xltm", ".odt", ".ott", ".ods", ".ots", ".pages", ".numbers", ".epub"];

const endsWithAny = (name: string, extensions: string[]) => {
  const lower = name.toLowerCase();
  return extensions.some((extension) => lower.endsWith(extension));
};

export function pagingOf(name: string): Paging | null {
  if (name.toLowerCase().endsWith(".pdf") || endsWithAny(name, LAID_OUT)) return "page";
  if (endsWithAny(name, PRESENTATIONS)) return "slide";
  return null;
}

export function atIsPage(name: string): boolean {
  return name.toLowerCase().endsWith(".pdf") || endsWithAny(name, PRESENTATIONS);
}

export function pagingWord(paging: Paging): string {
  return paging === "page" ? "Page" : "Slide";
}

export function pageFromText(text: string, pages: number | null): number | null {
  if (!/^\d{1,6}$/.test(text.trim())) return null;
  const page = Number(text.trim());
  return page >= 1 && (pages === null || page <= pages) ? page : null;
}

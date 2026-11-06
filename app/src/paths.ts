export function parentOf(path: string): string {
  const trimmed = path.replace(/[\\/]+$/, "");
  const cut = Math.max(trimmed.lastIndexOf("\\"), trimmed.lastIndexOf("/"));
  if (cut < 0) return "";
  const parent = trimmed.slice(0, cut);
  if (parent === "") return "/";
  return /^[A-Za-z]:$/.test(parent) ? `${parent}\\` : parent;
}

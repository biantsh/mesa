import { platform } from "./platform";

export function parentOf(path: string): string {
  const trimmed = path.replace(/[\\/]+$/, "");
  const cut = Math.max(trimmed.lastIndexOf("\\"), trimmed.lastIndexOf("/"));
  if (cut < 0) return "";
  const parent = trimmed.slice(0, cut);
  if (parent === "") return "/";
  return /^[A-Za-z]:$/.test(parent) ? `${parent}\\` : parent;
}

export const LINUX_ROOT_NAME = "File System";

export function nameOf(path: string, linux = platform === "linux"): string {
  if (linux && path === "/") return LINUX_ROOT_NAME;
  const trimmed = path.replace(/[\\/]+$/, "");
  const cut = Math.max(trimmed.lastIndexOf("\\"), trimmed.lastIndexOf("/"));
  return cut < 0 ? (trimmed === "" ? path : trimmed) : trimmed.slice(cut + 1);
}

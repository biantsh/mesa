import { execFileSync } from "node:child_process";
import app from "../../app/src-tauri/tauri.conf.json";

export interface Release {
  version: string;
  date: Date;
}

function git(...args: string[]): string {
  return execFileSync("git", args, { encoding: "utf8", stdio: ["ignore", "pipe", "ignore"] }).trim();
}

function latest(): Release {
  try {
    const lines = git("for-each-ref", "--sort=-v:refname", "--format=%(refname:short) %(creatordate:iso-strict)", "refs/tags/v[0-9]*").split("\n");
    const [tag, date] = (lines.find((line) => !line.split(" ")[0].includes("-")) ?? "").split(" ");
    if (tag && date) return { version: tag.slice(1), date: new Date(date) };
  } catch {
  }
  return { version: app.version, date: new Date() };
}

export const release = latest();

export function releaseLine({ version, date }: Release = release): string {
  const when = new Intl.DateTimeFormat("en", { month: "short", year: "numeric", timeZone: "UTC" }).format(date);
  return `v${version}, ${when}`;
}

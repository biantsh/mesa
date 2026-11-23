import { execFileSync } from "node:child_process";
import app from "../../app/src-tauri/tauri.conf.json";

export interface Release {
  version: string;
  date: Date;
}

function git(...args: string[]): string {
  return execFileSync("git", args, { encoding: "utf8", stdio: ["ignore", "pipe", "ignore"] }).trim();
}

function tagged(): Release | null {
  try {
    const lines = git("for-each-ref", "--sort=-v:refname", "--format=%(refname:short) %(creatordate:iso-strict)", "refs/tags/v[0-9]*").split("\n");
    const [tag, date] = (lines.find((line) => !line.split(" ")[0].includes("-")) ?? "").split(" ");
    if (tag && date) return { version: tag.slice(1), date: new Date(date) };
  } catch {
  }
  return null;
}

function latest(): Release {
  const found = tagged();
  if (found) return found;
  if (process.env.NODE_ENV === "production") {
    throw new Error(
      "No release tag (like v0.1.0) found in git, so the site can't say which version to download. " +
        "Tag a release first, or, in a shallow clone, fetch the tags with `git fetch --tags`.",
    );
  }
  return { version: app.version, date: new Date() };
}

export const release = latest();

export function releaseLine({ version, date }: Release = release): string {
  const when = new Intl.DateTimeFormat("en", { month: "short", year: "numeric", timeZone: "UTC" }).format(date);
  return `v${version}, ${when}`;
}

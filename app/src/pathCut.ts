export function pathParts(path: string): string[] {
  return path.split(/(?<=[\\/])/).filter((part) => part !== "");
}

function anchorLength(parts: string[]): number {
  const [root = "", second = ""] = parts.map((part) => part.toLowerCase());
  const windowsHome = /^[a-z]:[\\/]$/.test(root) && /^users[\\/]$/.test(second);
  const unixHome = root === "/" && /^(users|home)\/$/.test(second);
  return windowsHome || unixHome ? 4 : 2;
}

export function middleCuts(path: string): string[][] {
  const parts = pathParts(path);
  const separator = path.includes("\\") ? "\\" : "/";
  const anchor = Math.min(anchorLength(parts), parts.length);
  const cuts = [parts];
  for (let kept = parts.length - anchor - 1; kept >= 1; kept--) {
    cuts.push([...parts.slice(0, anchor), `…${separator}`, ...parts.slice(parts.length - kept)]);
  }
  return cuts;
}

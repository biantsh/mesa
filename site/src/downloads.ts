import { GITHUB } from "./links";
import { release } from "./release";

const FILES: Record<string, (version: string) => string> = {
  ".exe": (version) => `Mesa_${version}_x64-setup.exe`,
  ".dmg": (version) => `Mesa_${version}_aarch64.dmg`,
  ".deb": (version) => `Mesa_${version}_amd64.deb`,
  ".rpm": (version) => `Mesa-${version}-1.x86_64.rpm`,
  AppImage: (version) => `Mesa_${version}_amd64.AppImage`,
};

export function downloadUrl(kind: string, version = release.version): string {
  const file = FILES[kind];
  if (!file) throw new Error(`No release file is known for "${kind}"`);
  return `${GITHUB}/releases/download/v${version}/${file(version)}`;
}

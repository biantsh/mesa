import apple from "./assets/os/apple.svg?raw";
import windows from "./assets/os/windows.svg?raw";
import linux from "./assets/os/linux.svg?raw";

export type PlatformId = "mac" | "windows" | "linux";

export interface Platform {
  id: PlatformId;
  name: string;
  logo: string;
  needs: string;
  installers: string[];
}

export const PLATFORMS: Platform[] = [
  { id: "windows", name: "Windows", logo: windows, needs: "Windows 10 or 11 (x64)", installers: [".exe"] },
  { id: "mac", name: "macOS", logo: apple, needs: "macOS 13+ (Apple silicon)", installers: [".dmg"] },
  { id: "linux", name: "Linux", logo: linux, needs: "Ubuntu 22.04+ or equivalent (x64)", installers: [".deb", ".rpm", "AppImage"] },
];

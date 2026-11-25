import { spawnSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { createWriteStream, existsSync, mkdirSync, renameSync, rmSync } from 'node:fs';
import { arch, homedir, totalmem } from 'node:os';
import { delimiter, dirname, join, resolve } from 'node:path';
import { Readable } from 'node:stream';
import { pipeline } from 'node:stream/promises';
import { fileURLToPath } from 'node:url';

const CMAKE = {
  version: '3.31.6',
  sha256: {
    x64: '5a1133ff103c71eb5120e2cc3de922733e7d8a26a98ae716397e8676adb367bf',
    arm64: 'b4cc788d63112b2749b40627e719eb5d3b8ed8f00c36d77189f4019cfe64bc9e',
  },
  url: (v, a) => `https://github.com/Kitware/CMake/releases/download/v${v}/cmake-${v}-linux-${a === 'arm64' ? 'aarch64' : 'x86_64'}.tar.gz`,
};
const VULKAN_SDK = {
  version: '1.4.363.1',
  sha256: 'b7867d8ca658563813af666f34b93db87f1b1c7d9d69d97e924c210c7ba33eb8',
  url: (v) => `https://sdk.lunarg.com/sdk/download/${v}/linux/vulkansdk-linux-x86_64-${v}.tar.xz`,
};

export const cacheDir = join(process.env.XDG_CACHE_HOME || join(homedir(), '.cache'), 'mesa-build');
export const gstreamerPlugins = resolve(dirname(fileURLToPath(import.meta.url)), '..', 'src-tauri', 'target', 'gstreamer-plugins');

export async function cmake(minimum, log) {
  const atLeast = (command) => {
    const found = (capture(command, ['--version']).match(/version (\d+)\.(\d+)/) ?? []).slice(1).map(Number);
    return found.length === 2 && (found[0] > minimum[0] || (found[0] === minimum[0] && found[1] >= minimum[1])) ? found : null;
  };
  if (process.env.MESA_CMAKE) {
    if (!atLeast(process.env.MESA_CMAKE)) throw new Error(`MESA_CMAKE (${process.env.MESA_CMAKE}) isn't CMake ${minimum.join('.')} or newer`);
    return process.env.MESA_CMAKE;
  }
  if (atLeast('cmake')) return 'cmake';
  const a = arch();
  if (!CMAKE.sha256[a]) throw new Error(`CMake ${minimum.join('.')} or newer is needed, and there is none to download for ${a}: install it`);
  const dir = join(cacheDir, `cmake-${CMAKE.version}`);
  const bin = join(dir, 'bin', 'cmake');
  if (!existsSync(bin)) {
    log(`CMake ${minimum.join('.')} or newer isn't installed: downloading Kitware's ${CMAKE.version} (50 MB, once) into ${cacheDir}`);
    const archive = await download(CMAKE.url(CMAKE.version, a), CMAKE.sha256[a]);
    unpack(dir, (into) => ['-xzf', archive, '-C', into, '--strip-components=1']);
    rmSync(archive, { force: true });
  }
  return bin;
}

export async function vulkanSdk(log) {
  if (process.env.VULKAN_SDK && existsSync(join(process.env.VULKAN_SDK, 'bin', 'glslc'))) {
    return { sdk: process.env.VULKAN_SDK, bin: join(process.env.VULKAN_SDK, 'bin') };
  }
  const onPath = (process.env.PATH ?? '').split(delimiter).find((dir) => dir && existsSync(join(dir, 'glslc')));
  if (onPath) return { sdk: null, bin: onPath };
  if (arch() !== 'x64') {
    throw new Error('glslc, the shader compiler the Vulkan backend is built with, was not found: install it and the SPIR-V headers\n' +
      '  (the `glslc` and `spirv-headers` packages on Debian 12, Ubuntu 23.04 and Fedora; `shaderc` and `spirv-headers` on Arch), or\n' +
      '  use --backend=cpu for a build without GPU support');
  }
  const dir = join(cacheDir, `vulkan-sdk-${VULKAN_SDK.version}`);
  if (!existsSync(join(dir, 'bin', 'glslc'))) {
    log(`glslc isn't installed (Ubuntu 22.04 has no package for it): downloading the Vulkan SDK ${VULKAN_SDK.version} (367 MB, once; ` +
      `42 MB of it is kept) into ${cacheDir}`);
    const archive = await download(VULKAN_SDK.url(VULKAN_SDK.version), VULKAN_SDK.sha256);
    const parts = ['bin/glslc', 'include/vulkan', 'include/vk_video', 'include/spirv', 'share/cmake/SPIRV-Headers'];
    unpack(dir, (into) => ['-xJf', archive, '-C', into, '--strip-components=2', ...parts.map((p) => `${VULKAN_SDK.version}/x86_64/${p}`)]);
    rmSync(archive, { force: true });
  }
  return { sdk: dir, bin: join(dir, 'bin') };
}

export function buildJobs(cpuCount) {
  return Math.max(1, Math.min(cpuCount, Math.floor(totalmem() / (1.75 * 2 ** 30))));
}

export function checkSystem() {
  const needs = [
    ['git', 'command', 'git'],
    ['python3', 'command', 'python3'],
    ['c++', 'command', 'build-essential'],
    ['make', 'command', 'build-essential'],
    ['pkg-config', 'command', 'pkg-config'],
    ['file', 'command', 'file'],
    ['xz', 'command', 'xz-utils'],
    ['objdump', 'command', 'binutils'],
    ['webkit2gtk-4.1', 'pkg-config', 'libwebkit2gtk-4.1-dev'],
    ['openssl', 'pkg-config', 'libssl-dev'],
    ['gstreamer-1.0', 'pkg-config', 'libgstreamer1.0-dev'],
    ['gstreamer-app-1.0', 'pkg-config', 'libgstreamer-plugins-base1.0-dev'],
    ['gstreamer-video-1.0', 'pkg-config', 'libgstreamer-plugins-base1.0-dev'],
    ['libheif', 'pkg-config', 'libheif-dev'],
    ['vulkan', 'pkg-config', 'libvulkan-dev'],
    ['ayatana-appindicator3-0.1', 'pkg-config', 'libayatana-appindicator3-dev'],
    ['libgomp.so.1', 'library', 'libgomp1'],
  ];
  const missing = missingPackages(needs);
  if (missing) throw new Error(`building Mesa needs ${missing}`);
}

export function missingPackages(needs) {
  const hasPkgConfig = onPath('pkg-config');
  const missing = needs.filter(([name, how, , path]) => {
    if (how === 'command') return !onPath(name);
    if (how === 'pkg-config') return hasPkgConfig && spawnSync('pkg-config', ['--exists', name]).status !== 0;
    if (how === 'exists') return !existsSync(path);
    return !capture('/sbin/ldconfig', ['-p']).includes(name);
  });
  if (!missing.length) return null;
  const debian = existsSync('/etc/debian_version');
  const packages = [...new Set(missing.map(([, , pkg]) => pkg))];
  return (
    `some packages that aren't installed: ${missing.map(([name]) => name).join(', ')}.\n` +
    (debian
      ? `  Install them with:\n\n    sudo apt install ${packages.join(' ')}\n`
      : `  On Debian or Ubuntu they are: ${packages.join(' ')}. Install your distribution's packages for them.\n`)
  );
}

export async function download(url, sha256) {
  mkdirSync(cacheDir, { recursive: true });
  const file = join(cacheDir, `download-${sha256.slice(0, 12)}`);
  const response = await fetch(url);
  if (!response.ok || !response.body) throw new Error(`couldn't download ${url}: ${response.status} ${response.statusText}`);
  const hash = createHash('sha256');
  const body = Readable.fromWeb(response.body);
  body.on('data', (chunk) => hash.update(chunk));
  await pipeline(body, createWriteStream(file));
  const got = hash.digest('hex');
  if (got !== sha256) {
    rmSync(file, { force: true });
    throw new Error(`${url} isn't what was expected (SHA-256 ${got}, not ${sha256})`);
  }
  return file;
}

export function unpack(dir, tarArgs) {
  const partial = `${dir}.partial`;
  rmSync(partial, { recursive: true, force: true });
  mkdirSync(partial, { recursive: true });
  const result = spawnSync('tar', tarArgs(partial), { stdio: 'inherit' });
  if (result.status !== 0) throw new Error(`couldn't unpack into ${dir} (tar failed)`);
  rmSync(dir, { recursive: true, force: true });
  renameSync(partial, dir);
}

function onPath(command) {
  return spawnSync('which', [command], { stdio: 'ignore' }).status === 0;
}

function capture(command, args) {
  const result = spawnSync(command, args, { encoding: 'utf8' });
  return result.status === 0 ? result.stdout.trim() : '';
}

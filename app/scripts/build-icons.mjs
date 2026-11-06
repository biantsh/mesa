import { spawnSync } from 'node:child_process';
import { copyFileSync, existsSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const app = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const iconsDir = join(app, 'src-tauri', 'icons');
const source = join(iconsDir, 'mesa.svg');
const tauriCli = join(app, 'node_modules', '@tauri-apps', 'cli', 'tauri.js');

const keep = ['32x32.png', '64x64.png', '128x128.png', '128x128@2x.png', 'icon.png', 'icon.ico', 'icon.icns'];

if (!existsSync(source)) fail(`the logo isn't at ${source}`);
if (!existsSync(tauriCli)) fail(`the Tauri CLI isn't installed; run \`npm install\` in ${app} first`);

const scratch = mkdtempSync(join(tmpdir(), 'mesa-icons-'));
let problem = '';
try {
  const result = spawnSync(
    process.execPath,
    [tauriCli, 'icon', source, '--fit', 'contain', '--output', scratch],
    { cwd: app, encoding: 'utf8' },
  );
  if (result.status !== 0) {
    problem = `\`tauri icon\` failed:\n${result.stderr}${result.error ?? ''}`;
  } else {
    const missing = keep.find((name) => !existsSync(join(scratch, name)));
    if (missing) problem = `\`tauri icon\` didn't make ${missing}`;
    else for (const name of keep) copyFileSync(join(scratch, name), join(iconsDir, name));
  }
  if (!problem) problem = makeMacIcon();
} finally {
  rmSync(scratch, { recursive: true, force: true });
}
if (problem) fail(problem);
console.log(`build-icons: wrote ${keep.join(', ')} to ${iconsDir}`);

function makeMacIcon() {
  const logo = readFileSync(source, 'utf8');
  const view = /viewBox="([^"]+)"/.exec(logo)?.[1];
  const inner = /<svg[^>]*>([\s\S]*)<\/svg>/.exec(logo)?.[1];
  if (!view || !inner) return `couldn't read the logo in ${source}`;
  const [, , width, height] = view.split(/\s+/).map(Number);
  const room = 520;
  const scale = room / Math.max(width, height);
  const [w, h] = [width * scale, height * scale];
  const svg = `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 1024 1024" width="1024" height="1024">
  <defs><linearGradient id="plate" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="#2d221c"/><stop offset="1" stop-color="#1c1512"/></linearGradient></defs>
  <rect x="100" y="100" width="824" height="824" rx="185" fill="url(#plate)"/>
  <svg x="${(1024 - w) / 2}" y="${(1024 - h) / 2 + 12}" width="${w}" height="${h}" viewBox="${view}">${inner}</svg>
</svg>`;
  const folder = mkdtempSync(join(tmpdir(), 'mesa-mac-icon-'));
  try {
    const file = join(folder, 'mesa-macos.svg');
    writeFileSync(file, svg);
    const out = join(folder, 'out');
    const result = spawnSync(process.execPath, [tauriCli, 'icon', file, '--output', out], { cwd: app, encoding: 'utf8' });
    if (result.status !== 0 || !existsSync(join(out, 'icon.icns'))) return `\`tauri icon\` failed for the macOS icon:\n${result.stderr}`;
    copyFileSync(join(out, 'icon.icns'), join(iconsDir, 'icon.icns'));
    return '';
  } finally {
    rmSync(folder, { recursive: true, force: true });
  }
}

function fail(message) {
  console.error(`\nbuild-icons: ${message}\n`);
  process.exit(1);
}

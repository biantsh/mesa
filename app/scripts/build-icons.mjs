import { spawnSync } from 'node:child_process';
import { copyFileSync, existsSync, mkdtempSync, rmSync } from 'node:fs';
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
} finally {
  rmSync(scratch, { recursive: true, force: true });
}
if (problem) fail(problem);
console.log(`build-icons: wrote ${keep.join(', ')} to ${iconsDir}`);

function fail(message) {
  console.error(`\nbuild-icons: ${message}\n`);
  process.exit(1);
}

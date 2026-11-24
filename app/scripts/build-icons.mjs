import { spawnSync } from 'node:child_process';
import { copyFileSync, cpSync, existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
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
  if (!problem) problem = makeTrayTemplate();
  if (!problem) problem = makeLayeredIcon();
  if (!problem) problem = makeAssetsCar();
} finally {
  rmSync(scratch, { recursive: true, force: true });
}
if (problem) fail(problem);
console.log(`build-icons: wrote ${[...keep, 'tray-template.png', 'Mesa.icon', 'Assets.car'].join(', ')} to ${iconsDir}`);

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
  <defs>
    <linearGradient id="plate" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="#2d221c"/><stop offset="1" stop-color="#1c1512"/></linearGradient>
    <!-- The shadow Apple's template has under the plate: 10 pixels down, soft, 35% black. -->
    <filter id="shadow" x="-10%" y="-10%" width="120%" height="125%" color-interpolation-filters="sRGB">
      <feGaussianBlur in="SourceAlpha" stdDeviation="12"/>
      <feOffset dy="10" result="soft"/>
      <feComponentTransfer in="soft" result="shade"><feFuncA type="linear" slope="0.35"/></feComponentTransfer>
      <feMerge><feMergeNode in="shade"/><feMergeNode in="SourceGraphic"/></feMerge>
    </filter>
  </defs>
  <path d="${squircle(100, 100, 824, 184.3, 0.6)}" fill="url(#plate)" filter="url(#shadow)"/>
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

function squircle(x, y, size, radius, smoothing) {
  const p = (1 + smoothing) * radius;
  const arcMeasure = 90 * (1 - smoothing);
  const arcSection = Math.sin(((arcMeasure / 2) * Math.PI) / 180) * radius * Math.SQRT2;
  const alpha = (90 - arcMeasure) / 2;
  const p3p4 = radius * Math.tan(((alpha / 2) * Math.PI) / 180);
  const beta = 45 * smoothing;
  const c = p3p4 * Math.cos((beta * Math.PI) / 180);
  const d = c * Math.tan((beta * Math.PI) / 180);
  const b = (p - arcSection - c - d) / 3;
  const a = 2 * b;

  const centre = x + size / 2;
  const [right, top] = [x + size, y];
  const start = [right - p, top];
  const corner = [
    ['C', [start[0] + a, top], [start[0] + a + b, top], [start[0] + a + b + c, top + d]],
    ['A', [start[0] + a + b + c + arcSection, top + d + arcSection]],
    ['C', [start[0] + a + b + c + arcSection + d, top + d + arcSection + c], [start[0] + a + b + c + arcSection + d, top + d + arcSection + b + c], [right, top + p]],
  ];
  const turn = ([px, py], quarters) => {
    let [qx, qy] = [px, py];
    for (let i = 0; i < quarters; i++) [qx, qy] = [centre - (qy - centre), centre + (qx - centre)];
    return [qx, qy];
  };
  const f = ([px, py]) => `${px.toFixed(2)} ${py.toFixed(2)}`;
  let path = `M${f(start)}`;
  for (let quarter = 0; quarter < 4; quarter++) {
    const [sx, sy] = turn(start, quarter);
    if (quarter > 0) path += ` L${sx.toFixed(2)} ${sy.toFixed(2)}`;
    for (const [command, ...points] of corner) {
      const turned = points.map((point) => f(turn(point, quarter)));
      path += command === 'A' ? ` A${radius} ${radius} 0 0 1 ${turned[0]}` : ` C${turned.join(' ')}`;
    }
  }
  return `${path} Z`;
}

function makeLayeredIcon() {
  const logo = readFileSync(source, 'utf8');
  const view = /viewBox="([^"]+)"/.exec(logo)?.[1];
  const defs = /<defs>([\s\S]*?)<\/defs>/.exec(logo)?.[1];
  if (!view || !defs) return `couldn't read the logo in ${source}`;
  const [vx, vy, width, height] = view.split(/\s+/).map(Number);
  const room = 600;
  const scale = room / Math.max(width, height);
  const place = `translate(${((1024 - width * scale) / 2).toFixed(2)} ${((1024 - height * scale) / 2 + 14).toFixed(2)}) scale(${scale.toFixed(5)}) translate(${-vx} ${-vy})`;
  const tiers = {
    base: ['base_left', 'base_right'],
    band: ['band', 'band_right'],
    top: ['t1_left', 't1_front', 't1_mid', 't1_right', 'top'],
  };
  const folder = join(iconsDir, 'Mesa.icon');
  rmSync(folder, { recursive: true, force: true });
  mkdirSync(join(folder, 'Assets'), { recursive: true });
  const layers = [];
  for (const [name, ids] of Object.entries(tiers)) {
    const paths = ids.map((id) => new RegExp(`<path id="${id}"[^>]*/>`).exec(logo)?.[0]);
    if (paths.some((path) => !path)) return `couldn't find the ${name} of the logo in ${source}`;
    writeFileSync(
      join(folder, 'Assets', `${name}.svg`),
      `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 1024 1024" width="1024" height="1024">\n  <defs>${defs}</defs>\n  <g transform="${place}">\n    ${paths.join('\n    ')}\n  </g>\n</svg>\n`,
    );
    layers.push({ 'image-name': `${name}.svg`, name });
  }
  layers.reverse();
  const brown = [0x1c, 0x15, 0x12].map((channel) => (channel / 255).toFixed(5)).join(',');
  const icon = {
    fill: { 'automatic-gradient': `srgb:${brown},1.00000` },
    groups: [{ layers, shadow: { kind: 'neutral', opacity: 0.5 }, translucency: { enabled: false, value: 0 } }],
    'supported-platforms': { squares: ['macOS'] },
  };
  writeFileSync(join(folder, 'icon.json'), `${JSON.stringify(icon, null, 2)}\n`);
  return '';
}

function makeTrayTemplate() {
  const logo = readFileSync(source, 'utf8');
  const strength = { top: 1, t1_left: 1, t1_front: 1, t1_mid: 1, t1_right: 1, band: 0.62, band_right: 0.62, base_left: 0.36, base_right: 0.36 };
  let made = 0;
  const svg = logo.replace(/<path id="([a-z0-9_]+)"([^>]*?) fill="url\(#[^)]*\)"/g, (all, id, rest) => {
    if (!(id in strength)) return all;
    made++;
    return `<path id="${id}"${rest} fill="#000" fill-opacity="${strength[id]}"`;
  });
  if (made !== Object.keys(strength).length) return `couldn't find all ${Object.keys(strength).length} layers of the logo in ${source} (found ${made})`;
  const folder = mkdtempSync(join(tmpdir(), 'mesa-tray-icon-'));
  try {
    const file = join(folder, 'mesa-template.svg');
    writeFileSync(file, svg);
    const out = join(folder, 'out');
    const result = spawnSync(process.execPath, [tauriCli, 'icon', file, '--fit', 'contain', '--png', '72', '--output', out], { cwd: app, encoding: 'utf8' });
    if (result.status !== 0 || !existsSync(join(out, '72x72.png'))) return `\`tauri icon\` failed for the menu bar icon:\n${result.stderr}`;
    copyFileSync(join(out, '72x72.png'), join(iconsDir, 'tray-template.png'));
    return '';
  } finally {
    rmSync(folder, { recursive: true, force: true });
  }
}

function fail(message) {
  console.error(`\nbuild-icons: ${message}\n`);
  process.exit(1);
}

function makeAssetsCar() {
  if (process.platform !== 'darwin') return 'Assets.car can only be made on a Mac, with Xcode 26 or later';
  const env = { ...process.env };
  const actool = (args) => spawnSync('xcrun', ['actool', ...args], { encoding: 'utf8', env });
  const version = () => /short-bundle-version:\s*(\d+)/.exec(actool(['--version', '--output-format', 'human-readable-text']).stdout ?? '')?.[1];
  const xcode = '/Applications/Xcode.app/Contents/Developer';
  if (!version() && existsSync(xcode)) env.DEVELOPER_DIR = xcode;
  const major = Number(version());
  if (!major) return 'making Assets.car needs Xcode 26 or later, and actool wasn\'t found';
  if (major < 26) return `making Assets.car needs Xcode 26 or later, but actool is from Xcode ${major}`;
  const folder = mkdtempSync(join(tmpdir(), 'mesa-assets-car-'));
  try {
    const icon = join(folder, 'Icon.icon');
    const out = join(folder, 'out');
    cpSync(join(iconsDir, 'Mesa.icon'), icon, { recursive: true });
    mkdirSync(out);
    const result = actool([
      icon,
      '--compile', out,
      '--output-format', 'human-readable-text',
      '--output-partial-info-plist', join(out, 'assetcatalog_generated_info.plist'),
      '--app-icon', 'Icon',
      '--include-all-app-icons',
      '--enable-on-demand-resources', 'NO',
      '--development-region', 'en',
      '--target-device', 'mac',
      '--minimum-deployment-target', '26.0',
      '--platform', 'macosx',
    ]);
    if (result.status !== 0 || !existsSync(join(out, 'Assets.car'))) {
      return `actool couldn't compile Mesa.icon:\n${result.stdout ?? ''}${result.stderr ?? ''}`;
    }
    copyFileSync(join(out, 'Assets.car'), join(iconsDir, 'Assets.car'));
    return '';
  } finally {
    rmSync(folder, { recursive: true, force: true });
  }
}

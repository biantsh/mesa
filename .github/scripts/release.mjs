import { existsSync, readdirSync, readFileSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = fileURLToPath(new URL('../..', import.meta.url));
const version = JSON.parse(readFileSync(join(root, 'app', 'src-tauri', 'tauri.conf.json'), 'utf8')).version;

const UPDATES = [
  ['.app.tar.gz', ['darwin-aarch64', 'darwin-aarch64-app']],
  ['-setup.exe', ['windows-x86_64', 'windows-x86_64-nsis']],
  ['.AppImage', ['linux-x86_64', 'linux-x86_64-appimage']],
  ['.deb', ['linux-x86_64-deb']],
  ['.rpm', ['linux-x86_64-rpm']],
];

const [command, tag, dist] = process.argv.slice(2);
if (command !== 'check' && command !== 'manifest') {
  fail('usage: node .github/scripts/release.mjs check <tag> | manifest <tag> <folder>');
}
if (tag !== `v${version}` && !tag?.startsWith(`v${version}-`)) {
  fail(`the tag ${tag} doesn't match the app's version, ${version} (app/src-tauri/tauri.conf.json)`);
}
const notes = changelog(version);
if (!notes) fail(`CHANGELOG.md has no "## v${version}" section`);

if (command === 'check') {
  console.log(version);
} else {
  const base = `https://github.com/${process.env.GITHUB_REPOSITORY}/releases/download/${tag}`;
  const platforms = {};
  for (const name of readdirSync(dist).sort()) {
    const kind = UPDATES.find(([ending]) => name.endsWith(ending));
    if (!kind) continue;
    const sig = join(dist, `${name}.sig`);
    if (!existsSync(sig)) fail(`${name} has no .sig beside it`);
    for (const platform of kind[1]) {
      platforms[platform] = { signature: readFileSync(sig, 'utf8').trim(), url: `${base}/${encodeURIComponent(name)}` };
    }
  }
  const missing = UPDATES.flatMap(([, keys]) => keys).filter((key) => !platforms[key]);
  if (missing.length) fail(`no update package for ${missing.join(', ')} in ${dist}`);

  const manifest = { version, notes, pub_date: new Date().toISOString(), platforms };
  writeFileSync(join(dist, 'latest.json'), `${JSON.stringify(manifest, null, 2)}\n`);
  writeFileSync('notes.md', `${notes}\n`);
  console.log(`latest.json: ${Object.keys(platforms).join(', ')}`);
}

function changelog(version) {
  const lines = readFileSync(join(root, 'CHANGELOG.md'), 'utf8').split('\n');
  const start = lines.findIndex((line) => line.trim() === `## v${version}`);
  if (start < 0) return null;
  const end = lines.findIndex((line, at) => at > start && line.startsWith('## '));
  return lines.slice(start + 1, end < 0 ? undefined : end).join('\n').trim();
}

function fail(message) {
  console.error(`release: ${message}`);
  process.exit(1);
}

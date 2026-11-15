import { copyFileSync, existsSync, mkdirSync, readdirSync, rmSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const NEEDED = ['webgpu_dawn.dll', 'dxcompiler.dll'];
const IF_THERE = ['dxil.dll'];

if (process.platform !== 'win32') {
  process.exit(0);
}

const tauri = join(dirname(fileURLToPath(import.meta.url)), '..', 'src-tauri');
const target = process.env.CARGO_TARGET_DIR || join(tauri, 'target');
const profile = process.env.TAURI_ENV_DEBUG === 'true' ? 'debug' : 'release';
const triple = process.env.TAURI_ENV_TARGET_TRIPLE;
const built = [triple && join(target, triple, profile), join(target, profile)].find(
  (dir) => dir && existsSync(join(dir, NEEDED[0])),
);
if (!built) {
  console.error(
    `copy-ocr-runtime: ${NEEDED[0]} isn't beside the program that was built (looked in ${join(target, profile)}). ` +
      'It is put there when mesa-ocr is built for Windows with ort\'s "webgpu" and "copy-dylibs" features (src-tauri/ocr/Cargo.toml).',
  );
  process.exit(1);
}

const out = join(tauri, 'runtime', 'ocr');
rmSync(out, { recursive: true, force: true });
mkdirSync(out, { recursive: true });
for (const name of [...NEEDED, ...IF_THERE]) {
  const from = join(built, name);
  if (existsSync(from)) {
    copyFileSync(from, join(out, name));
  } else if (NEEDED.includes(name)) {
    console.error(`copy-ocr-runtime: ${name} isn't beside the program that was built, in ${built}`);
    process.exit(1);
  }
}
console.log(`copy-ocr-runtime: ${readdirSync(out).join(', ')} from ${built}`);

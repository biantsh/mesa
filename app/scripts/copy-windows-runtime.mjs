import { spawnSync } from 'node:child_process';
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
  fail(
    `${NEEDED[0]} isn't beside the program that was built (looked in ${join(target, profile)}). ` +
      'It is put there when mesa-ocr is built for Windows with ort\'s "webgpu" and "copy-dylibs" features (src-tauri/ocr/Cargo.toml).',
  );
}

const out = join(tauri, 'runtime', 'ocr');
rmSync(out, { recursive: true, force: true });
mkdirSync(out, { recursive: true });
for (const name of [...NEEDED, ...IF_THERE]) {
  const from = join(built, name);
  if (existsSync(from)) {
    copyFileSync(from, join(out, name));
  } else if (NEEDED.includes(name)) {
    fail(`${name} isn't beside the program that was built, in ${built}`);
  }
}

const llama = join(tauri, 'runtime', 'llama');
if (!existsSync(llama)) fail(`${llama} is missing: run \`npm run build:runtime\` first`);
const { crt, openmp } = visualCppRuntime();
const crtFiles = readdirSync(crt).filter((name) => name.endsWith('.dll'));
for (const name of crtFiles) {
  copyFileSync(join(crt, name), join(out, name));
  copyFileSync(join(crt, name), join(llama, name));
}
copyFileSync(join(openmp, 'vcomp140.dll'), join(llama, 'vcomp140.dll'));
console.log(`copy-windows-runtime: ${readdirSync(out).join(', ')}, and the Visual C++ runtime beside llama-server, from ${built} and ${crt}`);

function visualCppRuntime() {
  const vswhere = join(process.env['ProgramFiles(x86)'] ?? 'C:\\Program Files (x86)', 'Microsoft Visual Studio', 'Installer', 'vswhere.exe');
  const install = existsSync(vswhere)
    ? spawnSync(vswhere, ['-latest', '-products', '*', '-requires', 'Microsoft.VisualStudio.Component.VC.Tools.x86.x64', '-property', 'installationPath'], { encoding: 'utf8' }).stdout?.trim()
    : '';
  const redist = install ? join(install, 'VC', 'Redist', 'MSVC') : '';
  const versions = redist && existsSync(redist)
    ? readdirSync(redist)
        .filter((name) => /^\d+(\.\d+)+$/.test(name))
        .sort((a, b) => b.localeCompare(a, undefined, { numeric: true }))
    : [];
  for (const version of versions) {
    const x64 = join(redist, version, 'x64');
    const folders = existsSync(x64) ? readdirSync(x64) : [];
    const crt = folders.find((name) => /^Microsoft\.VC\d+\.CRT$/.test(name));
    const openmp = folders.find((name) => /^Microsoft\.VC\d+\.OpenMP$/.test(name));
    if (crt && openmp && existsSync(join(x64, openmp, 'vcomp140.dll'))) {
      return { crt: join(x64, crt), openmp: join(x64, openmp) };
    }
  }
  fail('the Visual C++ runtime to ship with Mesa was not found. Install Visual Studio or its Build Tools with the "Desktop development with C++" workload.');
}

function fail(message) {
  console.error(`copy-windows-runtime: ${message}`);
  process.exit(1);
}

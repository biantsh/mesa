import { spawnSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { copyFileSync, existsSync, mkdirSync, readFileSync, realpathSync, rmSync, symlinkSync, writeFileSync } from 'node:fs';
import { cpus } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const TAG = 'v1.31.0';
const COMMIT = '402979ac1bef89dcd55add41eaa7277c407599eb';
const CMAKE_MINIMUM = [3, 28];

const app = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const work = join(app, 'src-tauri', 'target', 'onnxruntime-build');
const source = join(work, 'source');
const build = join(work, 'build');
const out = join(app, 'src-tauri', 'runtime', 'onnxruntime');
const force = process.argv.slice(2).includes('--force');

if (process.platform !== 'linux') {
  process.exit(0);
}
if (process.env.MESA_SKIP_ONNXRUNTIME_BUILD) {
  console.log('build-onnxruntime: skipped (MESA_SKIP_ONNXRUNTIME_BUILD is set)');
  process.exit(0);
}

const flags = [
  '--config', 'Release',
  '--build_shared_lib',
  '--use_webgpu',
  '--no_telemetry',
  '--skip_tests',
  '--compile_no_warning_as_error',
  '--cmake_extra_defines', 'onnxruntime_BUILD_UNIT_TESTS=OFF',
];
const LAYOUT = 1;
const stamp = `${TAG} ${COMMIT} ${createHash('sha256').update(flags.join(' ')).digest('hex').slice(0, 12)} layout${LAYOUT}`;
const stampFile = join(out, 'BUILD');
const library = join(out, 'lib', 'libonnxruntime.so.1');

if (!force && existsSync(library) && existsSync(stampFile) && readFileSync(stampFile, 'utf8').trim() === stamp) {
  console.log(`build-onnxruntime: ONNX Runtime ${TAG} is built already`);
  process.exit(0);
}

const cmake = process.env.MESA_CMAKE || 'cmake';
const version = (capture(cmake, ['--version']).match(/version (\d+)\.(\d+)/) ?? []).slice(1).map(Number);
if (version.length !== 2) fail(`${cmake} wasn't found (CMake ${CMAKE_MINIMUM.join('.')} or newer is needed: see this script's header)`);
if (version[0] < CMAKE_MINIMUM[0] || (version[0] === CMAKE_MINIMUM[0] && version[1] < CMAKE_MINIMUM[1])) {
  fail(`${cmake} is CMake ${version.join('.')}; ONNX Runtime needs ${CMAKE_MINIMUM.join('.')} or newer (see this script's header)`);
}

console.log(`build-onnxruntime: building ONNX Runtime ${TAG} with WebGPU (about 20 minutes, once)`);

mkdirSync(work, { recursive: true });
if (!existsSync(join(source, '.git'))) {
  rmSync(source, { recursive: true, force: true });
  run('git', ['-c', 'advice.detachedHead=false', 'clone', '--quiet', '--depth', '1', '--branch', TAG,
    'https://github.com/microsoft/onnxruntime', source]);
}
const head = capture('git', ['-C', source, 'rev-parse', 'HEAD']);
if (head !== COMMIT) fail(`${source} is at ${head}, not ${COMMIT} (${TAG}); delete it and build again`);

const parallel = String(Math.max(1, cpus().length));
run('python3', [join(source, 'tools', 'ci_build', 'build.py'), '--build_dir', build, '--cmake_path', cmake,
  '--parallel', parallel, '--update', '--build', ...flags]);

const built = join(build, 'Release', 'libonnxruntime.so.1');
if (!existsSync(built)) fail(`${built} wasn't built (see the output above)`);
rmSync(out, { recursive: true, force: true });
mkdirSync(join(out, 'lib'), { recursive: true });
copyFileSync(realpathSync(built), library);
symlinkSync('libonnxruntime.so.1', join(out, 'lib', 'libonnxruntime.so'));
copyFileSync(join(source, 'LICENSE'), join(out, 'LICENSE-onnxruntime.txt'));
copyFileSync(join(source, 'ThirdPartyNotices.txt'), join(out, 'ThirdPartyNotices-onnxruntime.txt'));
writeFileSync(stampFile, `${stamp}\n`);

const needs = capture('objdump', ['-T', library]).match(/GLIBC_\d+\.\d+(\.\d+)?/g) ?? [];
const newest = needs.map((v) => v.slice('GLIBC_'.length)).sort((a, b) => a.localeCompare(b, undefined, { numeric: true })).pop();
console.log(`build-onnxruntime: done, ${library} (needs glibc ${newest ?? '?'} at most)`);

function run(command, args) {
  const result = spawnSync(command, args, { stdio: 'inherit' });
  if (result.error) fail(`${command} couldn't be run: ${result.error.message}`);
  if (result.status !== 0) fail(`${command} ${args.slice(0, 3).join(' ')}... failed (exit ${result.status})`);
}

function capture(command, args) {
  const result = spawnSync(command, args, { encoding: 'utf8' });
  return result.status === 0 ? result.stdout.trim() : '';
}

function fail(message) {
  console.error(`\nbuild-onnxruntime: ${message}\n`);
  process.exit(1);
}

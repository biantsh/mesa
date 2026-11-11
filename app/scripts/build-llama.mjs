import { spawnSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { copyFileSync, existsSync, mkdirSync, readdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { cpus } from 'node:os';
import { delimiter, dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const TAG = 'b12024';
const COMMIT = 'ddf48f85c933c1f438212a02eb21e7b001d2d37d';
const MACOS_MINIMUM = '13.0';

const app = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const work = join(app, 'src-tauri', 'target', 'llama-build');
const source = join(work, 'source');
const build = join(work, 'build');
const out = join(app, 'src-tauri', 'runtime', 'llama');
const isWindows = process.platform === 'win32';

const args = process.argv.slice(2);
const force = args.includes('--force');
const backend = (args.find((a) => a.startsWith('--backend=')) ?? '').slice('--backend='.length)
  || process.env.MESA_LLAMA_BACKEND
  || (process.platform === 'darwin' ? 'metal' : 'vulkan');

if (process.env.MESA_SKIP_LLAMA_BUILD) {
  console.log('build-llama: skipped (MESA_SKIP_LLAMA_BUILD is set)');
  process.exit(0);
}
if (!['vulkan', 'cpu', 'metal'].includes(backend)) fail(`unknown backend "${backend}" (vulkan, cpu or metal)`);

const flags = [
  '-DCMAKE_BUILD_TYPE=Release',
  '-DBUILD_SHARED_LIBS=ON',
  '-DGGML_BACKEND_DL=ON',
  '-DGGML_CPU_ALL_VARIANTS=ON',
  '-DGGML_NATIVE=OFF',
  backend === 'vulkan' ? '-DGGML_VULKAN=ON' : '-DGGML_VULKAN=OFF',
  backend === 'metal' ? '-DGGML_METAL=ON' : '-DGGML_METAL=OFF',
  '-DLLAMA_BUILD_TOOLS=ON',
  '-DLLAMA_BUILD_SERVER=ON',
  '-DLLAMA_BUILD_TESTS=OFF',
  '-DLLAMA_BUILD_EXAMPLES=OFF',
  '-DLLAMA_BUILD_APP=OFF',
  '-DLLAMA_BUILD_UI=OFF',
  '-DLLAMA_USE_PREBUILT_UI=OFF',
  '-DLLAMA_OPENSSL=OFF',
  '-DLLAMA_CURL=OFF',
  ...(process.platform === 'darwin'
    ? [
        `-DCMAKE_OSX_DEPLOYMENT_TARGET=${MACOS_MINIMUM}`,
        '-DCMAKE_OSX_ARCHITECTURES=arm64',
        '-DCMAKE_BUILD_WITH_INSTALL_RPATH=ON',
        '-DCMAKE_INSTALL_RPATH=@loader_path',
      ]
    : []),
  ...(process.platform === 'linux'
    ? ['-DCMAKE_BUILD_WITH_INSTALL_RPATH=ON', '-DCMAKE_INSTALL_RPATH=$ORIGIN']
    : []),
];
const LAYOUT = 3;
const signer = process.platform === 'darwin' ? process.env.APPLE_SIGNING_IDENTITY ?? '' : '';
const stamp = `${TAG} ${COMMIT} ${backend} ${createHash('sha256').update(flags.join(' ')).digest('hex').slice(0, 12)} layout${LAYOUT}${signer ? ` signed:${createHash('sha256').update(signer).digest('hex').slice(0, 8)}` : ''}`;
const stampFile = join(out, 'BUILD');
const program = join(out, isWindows ? 'llama-server.exe' : 'llama-server');

if (!force && existsSync(program) && existsSync(stampFile) && readFileSync(stampFile, 'utf8').trim() === stamp) {
  console.log(`build-llama: llama.cpp ${TAG} (${backend}) is built already`);
  process.exit(0);
}

console.log(`build-llama: building llama.cpp ${TAG} (${backend})`);
const tools = findTools();

mkdirSync(work, { recursive: true });
if (!existsSync(join(source, '.git'))) {
  rmSync(source, { recursive: true, force: true });
  run('git', ['-c', 'advice.detachedHead=false', '-c', 'core.autocrlf=false', 'clone', '--quiet', '--depth', '1',
    '--branch', TAG, 'https://github.com/ggml-org/llama.cpp', source]);
}
const head = capture('git', ['-C', source, 'rev-parse', 'HEAD']);
if (head !== COMMIT) fail(`${source} is at ${head}, not ${COMMIT} (${TAG}); delete it and build again`);

const parallel = String(Math.max(1, cpus().length));
tools.run([
  ['cmake', '-S', source, '-B', build, '-G', tools.generator, ...flags],
  ['cmake', '--build', build, '--config', 'Release', '--parallel', parallel],
]);

rmSync(out, { recursive: true, force: true });
mkdirSync(out, { recursive: true });
const bin = [join(build, 'bin'), join(build, 'bin', 'Release')].find((dir) => existsSync(dir));
if (!bin) fail(`nothing was built in ${build}\\bin`);
const files = readdirSync(bin).filter(
  (f) => f === (isWindows ? 'llama-server.exe' : 'llama-server')
    || (/\.(dll|so|dylib)(\.|$)/.test(f) && !/^(lib)?llama-(?!server).*-impl\./.test(f)),
);
if (!files.includes(isWindows ? 'llama-server.exe' : 'llama-server')) fail(`llama-server wasn't built (see the output above)`);
files.forEach((f) => copyFileSync(join(bin, f), join(out, f)));
copyFileSync(join(source, 'LICENSE'), join(out, 'LICENSE-llama.cpp.txt'));
const licences = join(source, 'licenses');
if (existsSync(licences)) {
  mkdirSync(join(out, 'licenses'), { recursive: true });
  readdirSync(licences).forEach((f) => copyFileSync(join(licences, f), join(out, 'licenses', f)));
}
if (signer) signMacBinaries(files);
writeFileSync(stampFile, `${stamp}\n`);
console.log(`build-llama: done, ${files.length} files in ${out}`);

function signMacBinaries(names) {
  console.log(`build-llama: signing ${names.length} files`);
  for (const name of names) {
    const args = ['--force', ...(signer === '-' ? [] : ['--options', 'runtime', '--timestamp']), '--sign', signer];
    run('codesign', [...args, join(out, name)]);
  }
}

function fail(message) {
  console.error(`\nbuild-llama: ${message}\n`);
  process.exit(1);
}

function run(command, commandArgs, options = {}) {
  const result = spawnSync(command, commandArgs, { stdio: 'inherit', ...options });
  if (result.error?.code === 'ENOENT') fail(`${command} isn't installed, or isn't on the PATH`);
  if (result.status !== 0) fail(`${command} ${commandArgs.slice(0, 2).join(' ')} failed`);
}

function capture(command, commandArgs) {
  const result = spawnSync(command, commandArgs, { encoding: 'utf8' });
  if (result.error?.code === 'ENOENT') fail(`${command} isn't installed, or isn't on the PATH`);
  return (result.stdout ?? '').trim();
}

function onPath(command) {
  return spawnSync(isWindows ? 'where' : 'which', [command], { stdio: 'ignore' }).status === 0;
}

function findTools() {
  if (!isWindows) {
    return {
      generator: onPath('ninja') ? 'Ninja' : 'Unix Makefiles',
      run: (commands) => commands.forEach(([command, ...rest]) => run(command, rest)),
    };
  }

  const extraPath = [];
  let vcvars = null;
  if (!onPath('cl')) {
    const vswhere = join(process.env['ProgramFiles(x86)'] ?? 'C:\\Program Files (x86)', 'Microsoft Visual Studio', 'Installer', 'vswhere.exe');
    const install = existsSync(vswhere)
      ? capture(vswhere, ['-latest', '-products', '*', '-requires', 'Microsoft.VisualStudio.Component.VC.Tools.x86.x64', '-property', 'installationPath'])
      : '';
    vcvars = install && join(install, 'VC', 'Auxiliary', 'Build', 'vcvars64.bat');
    if (!vcvars || !existsSync(vcvars)) {
      fail('no C++ compiler found. Install Visual Studio or its Build Tools with the "Desktop development with C++" workload.');
    }
    const cmakeFolder = join(install, 'Common7', 'IDE', 'CommonExtensions', 'Microsoft', 'CMake');
    extraPath.push(join(cmakeFolder, 'CMake', 'bin'), join(cmakeFolder, 'Ninja'));
  }

  const environment = [];
  if (backend === 'vulkan') {
    const sdk = findVulkanSdk();
    if (!sdk) {
      fail(
        'the Vulkan SDK was not found. It provides the shader compiler (glslc) that the Vulkan backend is built with.\n' +
          '  Install it from https://vulkan.lunarg.com/sdk/home (any recent version), and run this again;\n' +
          '  or set VULKAN_SDK to where it is; or use --backend=cpu for a build without GPU support.',
      );
    }
    environment.push(`set "VULKAN_SDK=${sdk}"`);
    extraPath.push(join(sdk, 'Bin'));
  }

  return {
    generator: 'Ninja',
    run(commands) {
      const lines = ['@echo off'];
      if (vcvars) lines.push(`call "${vcvars}" >nul || exit /b 1`);
      if (extraPath.length) lines.push(`set "PATH=${extraPath.join(delimiter)}${delimiter}%PATH%"`);
      lines.push(...environment);
      for (const command of commands) {
        lines.push(`${command.map((a) => `"${a}"`).join(' ')} || exit /b 1`);
      }
      mkdirSync(work, { recursive: true });
      const batch = join(work, 'build.bat');
      writeFileSync(batch, `${lines.join('\r\n')}\r\n`);
      run('cmd', ['/c', batch]);
    },
  };
}

function findVulkanSdk() {
  const candidates = [];
  if (process.env.VULKAN_SDK) candidates.push(process.env.VULKAN_SDK);
  if (existsSync('C:\\VulkanSDK')) {
    candidates.push(...readdirSync('C:\\VulkanSDK').sort().reverse().map((v) => join('C:\\VulkanSDK', v)));
  }
  return candidates.find((dir) => existsSync(join(dir, 'Bin', 'glslc.exe')));
}

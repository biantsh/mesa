import { spawnSync } from 'node:child_process';
import { existsSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const env = { ...process.env };
if (process.platform === 'linux') {
  const lib = join(resolve(dirname(fileURLToPath(import.meta.url)), '..'), 'src-tauri', 'runtime', 'onnxruntime', 'lib');
  if (!existsSync(join(lib, 'libonnxruntime.so'))) {
    console.error(`\ncargo.mjs: ONNX Runtime isn't built in ${lib}: run \`npm run build:runtime\` in app/ (see scripts/build-onnxruntime.mjs)\n`);
    process.exit(1);
  }
  env.ORT_LIB_LOCATION = lib;
  env.ORT_PREFER_DYNAMIC_LINK = '1';
}
const result = spawnSync(process.env.CARGO || 'cargo', process.argv.slice(2), { stdio: 'inherit', env });
if (result.error) {
  console.error(`cargo.mjs: cargo couldn't be run: ${result.error.message}`);
  process.exit(1);
}
process.exit(result.status ?? 1);

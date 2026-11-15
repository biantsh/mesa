#!/usr/bin/env node

import { spawnSync } from 'node:child_process';
import { existsSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { checkSystem } from './linux-tools.mjs';

const app = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const args = process.argv.slice(2);
const BUILDS = new Set(['build', 'b', 'run', 'r', 'test', 't', 'bench', 'check', 'c', 'clippy', 'doc', 'd', 'rustc', 'install']);
const command = args.find((a) => !a.startsWith('-') && !a.startsWith('+'));

const env = { ...process.env };
if (process.platform === 'linux') {
  if (BUILDS.has(command)) {
    try {
      checkSystem();
    } catch (e) {
      fail(e.message);
    }
    for (const script of ['build-llama.mjs', 'build-onnxruntime.mjs']) {
      const result = spawnSync(process.execPath, [join(app, 'scripts', script)], { stdio: ['inherit', 2, 2] });
      if (result.status !== 0) process.exit(result.status ?? 1);
    }
  }
  const lib = join(app, 'src-tauri', 'runtime', 'onnxruntime', 'lib');
  if (BUILDS.has(command) && !existsSync(join(lib, 'libonnxruntime.so'))) {
    fail(`ONNX Runtime isn't built in ${lib}: run \`npm run build:runtime\` in app/ (see scripts/build-onnxruntime.mjs)`);
  }
  env.ORT_LIB_LOCATION = lib;
  env.ORT_PREFER_DYNAMIC_LINK = '1';
}
const result = spawnSync(process.env.CARGO || 'cargo', args, { stdio: 'inherit', env });
if (result.error) fail(`cargo couldn't be run: ${result.error.message}`);
process.exit(result.status ?? 1);

function fail(message) {
  console.error(`\ncargo.mjs: ${message}\n`);
  process.exit(1);
}

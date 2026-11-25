#!/usr/bin/env node

import { spawnSync } from 'node:child_process';
import { createRequire } from 'node:module';
import { gstreamerPlugins } from './linux-tools.mjs';

const env = { ...process.env };
if (process.platform === 'linux') env.GSTREAMER_PLUGINS_DIR = gstreamerPlugins;
const cli = createRequire(import.meta.url).resolve('@tauri-apps/cli/tauri.js');
const result = spawnSync(process.execPath, [cli, ...process.argv.slice(2)], { stdio: 'inherit', env });
process.exit(result.status ?? 1);

#!/usr/bin/env node

import { execFileSync } from 'node:child_process';
import { existsSync, mkdirSync, readFileSync, readdirSync, statSync, writeFileSync } from 'node:fs';
import { basename, dirname, extname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const app = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const tauri = join(app, 'src-tauri');
const noticesDir = join(app, 'notices');
const out = join(tauri, 'runtime', 'notices.json');
const ORT_COMMIT = '402979ac1bef89dcd55add41eaa7277c407599eb';
const DAWN_TAG = 'v20260916.214021';
const DXC_COMMIT = '397ffb2264b4650c97b2418ec9a0f07228719317';
const SPIRV_HEADERS_COMMIT = '04fd3caa1e8267e4d95c806cad901181728e1006';
const VULKAN_HEADERS_COMMIT = 'ee2ec5fd83dafce291024683b50dc89219333076';
const VULKAN_UTILITY_COMMIT = '930a38bce146cf85c5bd7cb00fa33a66c640c0c6';
const PREFERRED = ['MIT', 'Apache-2.0', 'BSD-3-Clause', 'BSD-2-Clause', 'ISC'];
const STANDARD = new Set(readdirSync(join(noticesDir, 'standard')).map((f) => basename(f, '.txt')));
const NEEDS_COPYRIGHT_LINE = new Set(['Zlib']);

const triple = process.env.TAURI_ENV_TARGET_TRIPLE || hostTriple();
const os = triple.includes('windows') ? 'windows' : triple.includes('apple') ? 'macos' : 'linux';
const texts = [];
const textIndex = new Map();
function text(raw) {
  const lines = raw
    .replace(/^﻿/, '')
    .replace(/\r\n?/g, '\n')
    .split('\n')
    .map((line) => line.trimEnd());
  const margin = Math.min(...lines.filter(Boolean).map((line) => line.match(/^ */)[0].length));
  const clean = lines
    .map((line) => line.slice(margin))
    .join('\n')
    .replace(/^\n+|\n+$/g, '');
  const key = clean.replace(/\s+/g, ' ');
  let index = textIndex.get(key);
  if (index === undefined) {
    index = texts.length;
    texts.push(clean);
    textIndex.set(key, index);
  }
  return index;
}
const file = (...parts) => text(readFileSync(join(...parts), 'utf8'));
const bundled = (name) => file(noticesDir, 'bundled', name);
const standard = (id) => file(noticesDir, 'standard', `${id}.txt`);

function gather() {
  const mesaLicense = file(app, '..', 'LICENSE');
  return [
  { id: 'mesa', title: 'Mesa', items: [{ name: 'Mesa', license: 'MIT', texts: [mesaLicense] }] },
  {
    id: 'models',
    title: 'Models',
    items: [
      {
        name: 'EmbeddingGemma 2',
        by: 'Google DeepMind',
        license: 'Apache-2.0',
        note: 'Converted to GGUF and quantized to Q8_0 by ggml-org, and compressed with zstd for Mesa.',
        url: 'https://huggingface.co/google/embeddinggemma-2',
        texts: [standard('Apache-2.0')],
      },
      {
        name: 'PP-OCRv6 tiny',
        by: 'PaddlePaddle Authors; converted to ONNX by RapidOCR',
        license: 'Apache-2.0',
        note: 'The detector and recognizer that read the text in pictures, rewritten to run on the graphics card and compressed for Mesa.',
        url: 'https://github.com/PaddlePaddle/PaddleOCR',
        texts: [bundled('paddleocr-LICENSE.txt'), bundled('rapidocr-LICENSE.txt')],
      },
    ],
  },
  { id: 'programs', title: 'Included programs', items: programs() },
  { id: 'javascript', title: 'JavaScript packages', items: merge(npmPackages()) },
  { id: 'rust', title: 'Rust crates', items: merge(crates()) },
  ];
}

function programs() {
  const llama = join(tauri, 'runtime', 'llama');
  if (!existsSync(join(llama, 'LICENSE-llama.cpp.txt'))) {
    fail(`llama.cpp's license isn't in ${llama}: build it first (\`node scripts/build-llama.mjs\`, which \`npm run build:runtime\` runs)`);
  }
  const [tag, commit] = readFileSync(join(llama, 'BUILD'), 'utf8').trim().split(/\s+/);
  const llamaExtra = existsSync(join(llama, 'licenses'))
    ? readdirSync(join(llama, 'licenses')).sort().map((f) => file(llama, 'licenses', f))
    : [];
  const ortScript = readFileSync(join(app, 'scripts', 'build-onnxruntime.mjs'), 'utf8');
  const built = ortScript.match(/const COMMIT = '([0-9a-f]{40})'/)?.[1];
  if (built !== ORT_COMMIT) {
    fail(
      `scripts/build-onnxruntime.mjs builds ONNX Runtime at ${built}, but notices/bundled has its licenses from ${ORT_COMMIT}: ` +
        'update them from that commit and change ORT_COMMIT here',
    );
  }
  const items = [
    {
      name: 'llama.cpp',
      version: tag,
      by: 'The ggml authors',
      license: 'MIT',
      note: 'Runs the embedding model.',
      url: `https://github.com/ggml-org/llama.cpp/tree/${commit}`,
      texts: [file(llama, 'LICENSE-llama.cpp.txt'), ...llamaExtra],
    },
    {
      name: 'ONNX Runtime',
      version: ortScript.match(/const TAG = '([^']+)'/)?.[1],
      by: 'Microsoft Corporation',
      license: 'MIT',
      note: 'Runs the models that read the text in pictures. Its notices include those of Dawn, which runs it on the graphics card.',
      url: `https://github.com/microsoft/onnxruntime/tree/${ORT_COMMIT}`,
      texts: [bundled('onnxruntime-LICENSE.txt'), bundled('onnxruntime-ThirdPartyNotices.txt')],
    },
  ];
  if (os === 'windows') {
    items.push({
      name: 'DirectX Shader Compiler',
      by: 'Microsoft Corporation',
      license: 'NCSA',
      note: `Compiles Dawn's shaders for Direct3D 12 (dxcompiler.dll), as Dawn ${DAWN_TAG} pins it.`,
      url: `https://github.com/microsoft/DirectXShaderCompiler/tree/${DXC_COMMIT}`,
      texts: [bundled('dxc-LICENSE.txt'), bundled('dxc-ThirdPartyNotices.txt')],
    });
  }
  if (os === 'linux') {
    const apache = standard('Apache-2.0');
    const gstScript = readFileSync(join(app, 'scripts', 'build-gstreamer.mjs'), 'utf8');
    const [ffmpeg, gstLibav] = ['FFMPEG', 'GST_LIBAV'].map((name) => gstScript.match(new RegExp(`const ${name} = \\{\\s*version: '([^']+)'`))?.[1]);
    const lgpl = bundled('LGPL-2.1.txt');
    items.push(
      {
        name: 'FFmpeg',
        version: ffmpeg,
        by: 'The FFmpeg developers',
        license: 'LGPL-2.1-or-later',
        note: 'In the AppImage only: decodes H.264, H.265, AAC and other formats for GStreamer. Built from the unchanged source, as LGPL, by scripts/build-gstreamer.mjs.',
        url: `https://ffmpeg.org/releases/ffmpeg-${ffmpeg}.tar.xz`,
        texts: [lgpl],
      },
      {
        name: 'gst-libav',
        version: gstLibav,
        by: 'The GStreamer developers',
        license: 'LGPL-2.1-or-later',
        note: 'In the AppImage only: lets GStreamer use FFmpeg. Built from the unchanged source by scripts/build-gstreamer.mjs.',
        url: `https://gstreamer.freedesktop.org/src/gst-libav/gst-libav-${gstLibav}.tar.xz`,
        texts: [lgpl],
      },
      {
        name: 'SPIRV-Headers',
        by: 'The Khronos Group Inc.',
        license: 'MIT',
        note: 'Built into Dawn, for Vulkan.',
        url: `https://github.com/KhronosGroup/SPIRV-Headers/tree/${SPIRV_HEADERS_COMMIT}`,
        texts: [bundled('spirv-headers-LICENSE.txt')],
      },
      {
        name: 'Vulkan-Headers',
        by: 'The Khronos Group Inc.',
        license: 'Apache-2.0 OR MIT',
        note: 'Built into Dawn, for Vulkan.',
        url: `https://github.com/KhronosGroup/Vulkan-Headers/tree/${VULKAN_HEADERS_COMMIT}`,
        texts: [bundled('vulkan-headers-LICENSE.md'), apache],
      },
      {
        name: 'Vulkan-Utility-Libraries',
        by: 'The Khronos Group Inc., Valve Corporation, LunarG, Inc.',
        license: 'Apache-2.0',
        note: 'Built into Dawn, for Vulkan.',
        url: `https://github.com/KhronosGroup/Vulkan-Utility-Libraries/tree/${VULKAN_UTILITY_COMMIT}`,
        texts: [bundled('vulkan-utility-libraries-LICENSE.md'), apache],
      },
    );
  }
  return items;
}

function crates() {
  const metadata = JSON.parse(
    execFileSync('cargo', ['metadata', '--format-version', '1', '--locked', '--filter-platform', triple], {
      cwd: tauri,
      encoding: 'utf8',
      maxBuffer: 256 * 1024 * 1024,
      stdio: ['ignore', 'pipe', 'inherit'],
    }),
  );
  const packages = new Map(metadata.packages.map((p) => [p.id, p]));
  const nodes = new Map(metadata.resolve.nodes.map((n) => [n.id, n]));
  const isMacro = (id) => packages.get(id).targets.some((t) => t.kind.includes('proc-macro'));
  const own = metadata.packages.filter((p) => p.source == null).map((p) => p.id);
  const reached = new Set();
  const stack = [...own];
  while (stack.length) {
    const id = stack.pop();
    if (reached.has(id)) continue;
    reached.add(id);
    for (const dep of nodes.get(id)?.deps ?? []) {
      if (dep.dep_kinds.some((k) => k.kind == null) && !isMacro(dep.pkg)) stack.push(dep.pkg);
    }
  }
  const items = [];
  for (const id of reached) {
    const p = packages.get(id);
    if (p.source == null) continue;
    const expression = p.license ? normalizeExpression(p.license) : null;
    const texts = licenseTexts(dirname(p.manifest_path), expression, p.authors ?? [], p.name, p.license_file);
    items.push({
      name: p.name,
      version: p.version,
      license: expression ?? 'See the license file',
      url: `https://crates.io/crates/${p.name}/${p.version}`,
      texts,
    });
  }
  return items.sort(byName);
}

function npmPackages() {
  const pkg = JSON.parse(readFileSync(join(app, 'package.json'), 'utf8'));
  const found = new Map();
  const stack = Object.keys(pkg.dependencies ?? {}).map((name) => [name, app]);
  while (stack.length) {
    const [name, from] = stack.pop();
    const dir = moduleDir(name, from);
    if (found.has(dir)) continue;
    const p = JSON.parse(readFileSync(join(dir, 'package.json'), 'utf8'));
    const expression = typeof p.license === 'string' ? normalizeExpression(p.license) : null;
    const authors = [p.author?.name ?? p.author].filter((a) => typeof a === 'string');
    const texts = licenseTexts(dir, expression, authors, p.name);
    found.set(dir, {
      name: p.name,
      version: p.version,
      license: expression ?? 'See the license file',
      url: `https://www.npmjs.com/package/${p.name}/v/${p.version}`,
      texts,
    });
    for (const dep of Object.keys(p.dependencies ?? {})) stack.push([dep, dir]);
  }
  return [...found.values()].sort(byName);
}

function moduleDir(name, from) {
  for (let dir = from; ; dir = dirname(dir)) {
    const candidate = join(dir, 'node_modules', name);
    if (existsSync(join(candidate, 'package.json'))) return candidate;
    if (dirname(dir) === dir) fail(`${name} (needed from ${from}) isn't in node_modules: run \`npm install\``);
  }
}

const LICENSE_NAME = /^(licen[cs]e|copying|copyright|notice|unlicense|ofl|third[-_]?party)/i;
const NOT_TEXT = new Set(['.rs', '.toml', '.json', '.spdx', '.py', '.c', '.h', '.js', '.mjs', '.cjs', '.ts', '.yml', '.yaml', '.lock', '.orig', '.sh', '.html', '.svg', '.png']);
const SKIP_DIRS = new Set(['tests', 'test', 'testdata', 'benches', 'examples', 'fuzz', 'target', 'node_modules', '.git', '.github', 'src']);

function licenseFiles(dir, depth = 0) {
  const found = [];
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    const path = join(dir, entry.name);
    if (entry.isDirectory()) {
      if (depth < 2 && !SKIP_DIRS.has(entry.name.toLowerCase())) found.push(...licenseFiles(path, depth + 1).map((f) => ({ ...f, nested: true })));
    } else if (LICENSE_NAME.test(entry.name) && !NOT_TEXT.has(extname(entry.name).toLowerCase()) && statSync(path).size > 0) {
      found.push({ path, name: entry.name, nested: false });
    }
  }
  return found.sort((a, b) => a.path.localeCompare(b.path));
}

function licenseTexts(dir, expression, authors, name, licenseFile) {
  const files = licenseFiles(dir).map((f) => ({ ...f, body: readFileSync(f.path, 'utf8') }));
  if (licenseFile && existsSync(join(dir, licenseFile)) && !files.some((f) => f.path === join(dir, licenseFile))) {
    files.unshift({ path: join(dir, licenseFile), name: basename(licenseFile), nested: false, body: readFileSync(join(dir, licenseFile), 'utf8') });
  }
  const top = files.filter((f) => !f.nested).map((f) => ({ ...f, kind: classify(f.name, f.body) }));
  const have = new Set(top.flatMap((f) => idsOf(f.kind)));
  const chosen = expression ? choose(parse(expression), have) : [];
  const out = [];
  for (const f of top) {
    if (f.kind === 'NOTICE' || f.kind == null || chosen.some((id) => idsOf(f.kind).includes(id)) || chosen.length === 0) out.push(text(f.body));
  }
  for (const id of chosen) {
    if (have.has(id) || !STANDARD.has(id)) continue;
    out.push(text(standardFor(id, authors, name)));
  }
  const nested = files.filter((f) => f.nested).map((f) => ({ ...f, kind: classify(f.name, f.body) }));
  for (const f of nested) {
    const besideOther = nested.some((g) => dirname(g.path) === dirname(f.path) && g.kind && g.kind !== 'GPL' && g.kind !== 'NOTICE');
    if (f.kind === 'GPL' && besideOther) continue;
    out.push(text(f.body));
  }
  return [...new Set(out)];
}

function standardFor(id, authors, name) {
  const holders = authors.map((a) => a.replace(/\s*<[^>]*>/, '').trim()).filter(Boolean).join(', ') || `The ${name} authors`;
  const body = readFileSync(join(noticesDir, 'standard', `${id}.txt`), 'utf8');
  const filled = body.replace(
    /<year> <(copyright holders|owner)>|<YEAR> <COPYRIGHT HOLDER>|YEAR by AUTHOR EMAIL|2004-2010 by Internet Systems Consortium, Inc\. \("ISC"\)/,
    holders,
  );
  return NEEDS_COPYRIGHT_LINE.has(id) ? `Copyright (c) ${holders}\n\n${filled}` : filled;
}

function classify(name, body) {
  const n = name.toUpperCase();
  if (n.startsWith('NOTICE') || n.startsWith('THIRD')) return 'NOTICE';
  if (/MIT-0/.test(n)) return 'MIT-0';
  if (/(^|[-_.])MIT\b/.test(n)) return 'MIT';
  if (/APACHE/.test(n)) return 'Apache-2.0';
  if (/ZLIB/.test(n)) return 'Zlib';
  if (/UNLICENSE/.test(n)) return 'Unlicense';
  if (/BSL|BOOST/.test(n)) return 'BSL-1.0';
  if (/CC0/.test(n)) return 'CC0-1.0';
  if (/0BSD/.test(n)) return '0BSD';
  if (/BSD/.test(n)) return 'BSD';
  if (/MPL/.test(n)) return 'MPL-2.0';
  if (/UNICODE/.test(n)) return 'Unicode-3.0';
  if (/GNU (LESSER |LIBRARY |AFFERO )?GENERAL PUBLIC LICENSE/i.test(body)) return 'GPL';
  if (/Apache License/i.test(body) && /Version 2\.0/.test(body)) return 'Apache-2.0';
  if (/Mozilla Public License/i.test(body)) return 'MPL-2.0';
  if (/free and unencumbered software released into the public domain/i.test(body)) return 'Unlicense';
  if (/Boost Software License/i.test(body)) return 'BSL-1.0';
  if (/UNICODE LICENSE V3|UNICODE, INC\. LICENSE AGREEMENT/i.test(body)) return 'Unicode-3.0';
  if (/Community Data License Agreement/i.test(body)) return 'CDLA-Permissive-2.0';
  if (/CC0 1\.0 Universal/i.test(body)) return 'CC0-1.0';
  if (/SIL OPEN FONT LICENSE/i.test(body)) return 'OFL-1.1';
  if (/Permission is hereby granted, free of charge/i.test(body)) return 'MIT';
  if (/Redistribution and use in source and binary forms/i.test(body)) return 'BSD';
  if (/Permission to use, copy, modify, and\/or distribute this software for any/i.test(body)) return 'ISC';
  if (/provided ['‘]as-is['’]/i.test(body) && /altered source versions/i.test(body)) return 'Zlib';
  return null;
}

function idsOf(kind) {
  if (kind === 'BSD') return ['BSD-3-Clause', 'BSD-2-Clause'];
  if (kind === 'ISC') return ['ISC', '0BSD'];
  if (kind === 'MIT') return ['MIT', 'MIT-0'];
  return kind && kind !== 'NOTICE' ? [kind] : [];
}

function normalizeExpression(expression) {
  return expression.replace(/\s*\/\s*/g, ' OR ').replace(/\s+/g, ' ').trim();
}

function parse(expression) {
  const tokens = expression.replace(/\(/g, ' ( ').replace(/\)/g, ' ) ').split(/\s+/).filter(Boolean);
  let i = 0;
  const or = () => {
    const options = [and()];
    while (tokens[i]?.toUpperCase() === 'OR') {
      i++;
      options.push(and());
    }
    return options.length === 1 ? options[0] : { or: options };
  };
  const and = () => {
    const all = [atom()];
    while (tokens[i]?.toUpperCase() === 'AND') {
      i++;
      all.push(atom());
    }
    return all.length === 1 ? all[0] : { and: all };
  };
  const atom = () => {
    if (tokens[i] === '(') {
      i++;
      const inner = or();
      i++;
      return inner;
    }
    const id = (tokens[i++] ?? '').replace(/\+$/, '');
    if (tokens[i]?.toUpperCase() === 'WITH') i += 2;
    return { id };
  };
  return or();
}

function choose(node, have) {
  if (node.id) return [node.id];
  if (node.and) return [...new Set(node.and.flatMap((n) => choose(n, have)))];
  const rank = (id) => (PREFERRED.includes(id) ? PREFERRED.indexOf(id) : PREFERRED.length);
  const score = (ids) => (ids.every((id) => have.has(id)) ? 0 : 1000) + Math.max(...ids.map(rank));
  return node.or.map((n) => choose(n, have)).sort((a, b) => score(a) - score(b))[0];
}

function merge(items) {
  const merged = new Map();
  for (const item of items) {
    const key = `${item.name}\n${item.license}\n${item.texts.join(',')}`;
    const there = merged.get(key);
    if (there) there.version += `, ${item.version}`;
    else merged.set(key, { ...item });
  }
  return [...merged.values()];
}

function byName(a, b) {
  return a.name.localeCompare(b.name) || a.version.localeCompare(b.version, undefined, { numeric: true });
}

function hostTriple() {
  const info = execFileSync('rustc', ['-vV'], { encoding: 'utf8' });
  return info.match(/^host: (.+)$/m)?.[1].trim() ?? fail("rustc -vV didn't say the host's target");
}

function fail(message) {
  console.error(`build-notices: ${message}`);
  process.exit(1);
}

const sections = gather();
const json = JSON.stringify({ platform: os, texts, sections });
const before = existsSync(out) ? readFileSync(out, 'utf8') : null;
const count = sections.reduce((n, s) => n + s.items.length, 0);
if (before === json) {
  console.log(`build-notices: ${count} items for ${os}, as they were`);
} else {
  mkdirSync(dirname(out), { recursive: true });
  writeFileSync(out, json);
  console.log(`build-notices: ${count} items for ${os} (${(json.length / 1024).toFixed(0)} KB, ${texts.length} texts) in ${out}`);
}

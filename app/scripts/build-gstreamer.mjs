import { spawnSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { existsSync, mkdirSync, readFileSync, rmSync, symlinkSync, writeFileSync } from 'node:fs';
import { cpus } from 'node:os';
import { basename, dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { buildJobs, download, gstreamerPlugins as plugins, missingPackages, unpack } from './linux-tools.mjs';

const FFMPEG = {
  version: '4.4.5',
  sha256: 'f9514e0d3515aee5a271283df71636e1d1ff7274b15853bcd84e144be416ab07',
  url: (v) => `https://ffmpeg.org/releases/ffmpeg-${v}.tar.xz`,
};
const GST_LIBAV = {
  version: '1.20.3',
  sha256: '3fedd10560fcdfaa1b6462cbf79a38c4e7b57d7f390359393fc0cef6dbf27dfe',
  url: (v) => `https://gstreamer.freedesktop.org/src/gst-libav/gst-libav-${v}.tar.xz`,
};
const DECODERS = [
  'h264', 'hevc', 'mpeg4', 'h263', 'msmpeg4v1', 'msmpeg4v2', 'msmpeg4v3', 'wmv1', 'wmv2', 'wmv3', 'vc1',
  'mpeg1video', 'mpeg2video', 'mjpeg', 'prores', 'dvvideo',
  'aac', 'aac_latm', 'ac3', 'eac3', 'dca', 'alac', 'amrnb', 'amrwb', 'wmav1', 'wmav2', 'wmapro',
];
const PLUGINS = {
  'libgstreamer1.0-0': ['coreelements'],
  'gstreamer1.0-plugins-base': ['app', 'audioconvert', 'audioresample', 'ogg', 'opus', 'pbtypes', 'playback', 'theora',
    'typefindfunctions', 'videoconvert', 'videoscale', 'volume', 'vorbis'],
  'gstreamer1.0-gl': ['opengl'],
  'gstreamer1.0-alsa': ['alsa'],
  'gstreamer1.0-plugins-good': ['apetag', 'audiofx', 'audioparsers', 'autodetect', 'avi', 'flac', 'id3demux', 'isomp4',
    'matroska', 'mpg123', 'pulseaudio', 'videofilter', 'vpx', 'wavparse'],
  'gstreamer1.0-plugins-bad': ['aiff', 'aom', 'mpegpsdemux', 'mpegtsdemux', 'videoparsersbad'],
  'gstreamer1.0-plugins-ugly': ['asf'],
};

const app = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const work = join(app, 'src-tauri', 'target', 'gstreamer-build');
const out = join(app, 'src-tauri', 'runtime', 'gstreamer');
const lib = join(out, 'lib');
const libav = join(lib, 'gstreamer-1.0', 'libgstlibav.so');

if (process.platform !== 'linux') process.exit(0);
if (process.env.GSTREAMER_PLUGINS_DIR !== plugins) {
  fail(`GSTREAMER_PLUGINS_DIR isn't ${plugins}, so the AppImage would carry every GStreamer plugin on this computer, GPL ones too.\n` +
    '  Build with `npm run tauri build` in app/, which sets it.');
}

const system = capture('pkg-config', ['--variable=pluginsdir', 'gstreamer-1.0']);
const missing = missingPackages([
  ['meson', 'command', 'meson'],
  ['ninja', 'command', 'ninja-build'],
  ['nasm', 'command', 'nasm'],
  ['patchelf', 'command', 'patchelf'],
  ...Object.entries(PLUGINS).flatMap(([pkg, names]) => names.map((name) => [`libgst${name}.so`, 'exists', pkg, join(system, `libgst${name}.so`)])),
]);
if (missing) fail(`the AppImage's GStreamer plugins need ${missing}`);

const configure = [
  '--enable-shared', '--disable-static', '--disable-programs', '--disable-doc', '--disable-debug',
  '--disable-autodetect', '--enable-pthreads', '--disable-everything', '--disable-avdevice', '--disable-swscale',
  '--disable-network', `--enable-decoder=${DECODERS.join(',')}`,
];
const LAYOUT = 1;
const stamp = `ffmpeg ${FFMPEG.version} gst-libav ${GST_LIBAV.version} ` +
  `${createHash('sha256').update(configure.join(' ')).digest('hex').slice(0, 12)} layout${LAYOUT}`;
const stampFile = join(out, 'BUILD');
const log = (message) => console.log(`build-gstreamer: ${message}`);

if (existsSync(libav) && existsSync(stampFile) && readFileSync(stampFile, 'utf8').trim() === stamp) {
  log(`FFmpeg ${FFMPEG.version} (LGPL) and gst-libav ${GST_LIBAV.version} are built already`);
} else {
  log(`building FFmpeg ${FFMPEG.version} as LGPL, with ${DECODERS.length} decoders, and gst-libav ${GST_LIBAV.version} (a few minutes, once)`);
  const jobs = String(buildJobs(cpus().length));
  rmSync(out, { recursive: true, force: true });

  const ffmpeg = await source(FFMPEG, 'ffmpeg');
  run('./configure', [`--prefix=${out}`, `--extra-ldflags=-Wl,-rpath,${lib}`, ...configure], ffmpeg);
  const license = readFileSync(join(ffmpeg, 'config.h'), 'utf8').match(/#define FFMPEG_LICENSE "([^"]+)"/)?.[1];
  if (license !== 'LGPL version 2.1 or later') fail(`FFmpeg was configured as "${license}", not LGPL`);
  run('make', ['-j', jobs], ffmpeg);
  run('make', ['install'], ffmpeg);

  const gstLibav = await source(GST_LIBAV, 'gst-libav');
  const build = join(gstLibav, 'build');
  rmSync(build, { recursive: true, force: true });
  const env = { ...process.env, PKG_CONFIG_PATH: [join(lib, 'pkgconfig'), process.env.PKG_CONFIG_PATH].filter(Boolean).join(':') };
  run('meson', ['setup', build, gstLibav, `--prefix=${out}`, '--libdir=lib', '--buildtype=release', '-Ddoc=disabled',
    '-Dtests=disabled', `-Dc_link_args=-Wl,-rpath,${lib}`], gstLibav, env);
  run('ninja', ['-C', build, 'install'], gstLibav);
  writeFileSync(stampFile, `${stamp}\n`);
}

const linked = capture('ldd', [libav]);
const strays = linked.split('\n').filter((line) => /lib(av|sw)\w+\.so/.test(line) && !line.includes(`=> ${lib}/`));
if (strays.length) fail(`gst-libav finds FFmpeg libraries other than the LGPL ones built here:\n  ${strays.join('\n  ').trim()}`);

rmSync(plugins, { recursive: true, force: true });
mkdirSync(plugins, { recursive: true });
const files = [...Object.values(PLUGINS).flat().map((name) => join(system, `libgst${name}.so`)), libav];
for (const file of files) symlinkSync(file, join(plugins, basename(file)));
log(`${files.length} plugins for the AppImage in ${plugins}`);

async function source({ version, sha256, url }, name) {
  const dir = join(work, `${name}-${version}`);
  if (!existsSync(join(dir, '.complete'))) {
    log(`downloading ${name} ${version}`);
    const archive = await download(url(version), sha256).catch((e) => fail(e.message));
    unpack(dir, (into) => ['-xJf', archive, '-C', into, '--strip-components=1']);
    rmSync(archive, { force: true });
    writeFileSync(join(dir, '.complete'), '');
  }
  return dir;
}

function run(command, args, cwd, env = process.env) {
  const result = spawnSync(command, args, { cwd, env, stdio: ['ignore', 'inherit', 'inherit'] });
  if (result.error) fail(`${command} couldn't be run: ${result.error.message}`);
  if (result.status !== 0) fail(`${command} ${args.slice(0, 2).join(' ')}... failed (exit ${result.status})`);
}

function capture(command, args) {
  const result = spawnSync(command, args, { encoding: 'utf8' });
  return result.status === 0 ? result.stdout.trim() : '';
}

function fail(message) {
  console.error(`\nbuild-gstreamer: ${message}\n`);
  process.exit(1);
}

/**
 * Development helper: render the phone's first-run intro outside the app.
 *
 * The intro is a Remotion composition (src/components/intro) that the app
 * plays live with @remotion/player. This renders the same composition to
 * still frames or to an MP4, so it can be looked at frame by frame -- and
 * shown to someone -- without building the Android app.
 *
 *   node scripts/intro-video/render.mjs --frames 0,45,90        stills (PNG)
 *   node scripts/intro-video/render.mjs --sheet                 one contact sheet
 *   node scripts/intro-video/render.mjs --video                 the whole thing
 *
 * Options: --lang tr|en (tr)  --theme dark|light (dark)  --size 390x844
 *          --every N (sheet: one frame in N, default 15)  --out <dir>
 *          --scene <name>  one scene alone, from src/components/intro/scenes/<name>.tsx,
 *                          its frames counted from its own start
 *
 * Output goes to scripts/intro-video/out/, which git ignores. The first render
 * downloads Remotion's headless Chrome.
 */
import { execFileSync } from 'node:child_process';
import { mkdirSync, rmSync, writeFileSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

import { bundle } from '@remotion/bundler';
import { renderFrames, renderMedia, selectComposition } from '@remotion/renderer';
import { enableTailwind } from '@remotion/tailwind-v4';

const here = path.dirname(fileURLToPath(import.meta.url));
const root = path.resolve(here, '../..');

function option(name, fallback) {
  const at = process.argv.indexOf(`--${name}`);
  if (at < 0) return fallback;
  const value = process.argv[at + 1];
  return value === undefined || value.startsWith('--') ? true : value;
}

const lang = option('lang', 'tr');
const theme = option('theme', 'dark');
const [width, height] = String(option('size', '390x844')).split('x').map(Number);
const out = path.resolve(root, option('out', path.join(here, 'out')));
const scene = option('scene', null);
const tag = `${scene ? `${scene}-` : ''}${lang}-${theme}-${width}x${height}`;

mkdirSync(out, { recursive: true });

// A scene alone is bundled from an entry of its own that imports nothing but
// that scene, so a scene being written elsewhere cannot break this render.
let entryPoint = path.join(here, 'index.ts');
if (scene) {
  const entries = path.join(here, 'out', '.entries');
  mkdirSync(entries, { recursive: true });
  entryPoint = path.join(entries, `${scene}.ts`);
  writeFileSync(
    entryPoint,
    [
      "import { registerRoot } from 'remotion';",
      "import { rootFor } from '../../frame';",
      `import { scene } from '@/components/intro/scenes/${scene}';`,
      'registerRoot(rootFor(scene.Component, scene.duration));',
      '',
    ].join('\n'),
  );
}

const serveUrl = await bundle({
  entryPoint,
  // Webpack's disk cache is shared by every run, and two runs at once -- or
  // one after an edit -- left it unreadable ("reading 'length'" in wasm-hash).
  enableCaching: false,
  webpackOverride: (config) =>
    enableTailwind({
      ...config,
      resolve: {
        ...config.resolve,
        alias: { ...config.resolve?.alias, '@': path.join(root, 'src') },
      },
    }),
});

const inputProps = { lang, theme, width, height };
const composition = await selectComposition({ serveUrl, id: 'intro', inputProps });

// Rendered at the phone's own pixel density, so text is judged as it will be seen.
const scale = 3;

if (option('video', false)) {
  const file = path.join(out, `intro-${tag}.mp4`);
  await renderMedia({
    serveUrl,
    composition,
    inputProps,
    codec: 'h264',
    scale,
    outputLocation: file,
    onProgress: ({ progress }) => process.stdout.write(`\r${Math.round(progress * 100)}%`),
  });
  process.stdout.write(`\n${file}\n`);
} else {
  const sheet = Boolean(option('sheet', false));
  let frames;
  if (sheet) {
    const every = Number(option('every', 15));
    frames = [];
    for (let frame = 0; frame < composition.durationInFrames; frame += every) frames.push(frame);
    if (frames.at(-1) !== composition.durationInFrames - 1) frames.push(composition.durationInFrames - 1);
  } else {
    frames = String(option('frames', '0'))
      .split(',')
      .map((value) => Number(value.trim()));
  }

  // One browser for every frame, each written under a name of its own: a
  // sheet's cells numbered in order, a still by its frame.
  const cells = path.join(out, `sheet-${tag}`);
  if (sheet) {
    rmSync(cells, { recursive: true, force: true });
    mkdirSync(cells, { recursive: true });
  }
  const written = [];
  await renderFrames({
    serveUrl,
    composition,
    inputProps,
    frames,
    imageFormat: 'png',
    scale: sheet ? 1 : scale,
    outputDir: null,
    onStart: () => {},
    onFrameUpdate: () => {},
    onFrameBuffer: (buffer, frame) => {
      const file = sheet
        ? path.join(cells, `${String(frames.indexOf(frame)).padStart(3, '0')}.png`)
        : path.join(out, `frame-${tag}-${String(frame).padStart(4, '0')}.png`);
      writeFileSync(file, buffer);
      written.push(file);
    },
  });

  if (!sheet) {
    for (const file of written.sort()) console.log(file);
  } else {
    // Tiled with Pillow, which scripts/generate_icon.py already needs: the
    // FFmpeg that Remotion carries is built without the tiling filters. Handed
    // the folder rather than its files: a sheet of every other frame names
    // more of them than a Windows command line holds.
    const file = path.join(out, `sheet-${tag}.png`);
    const tile = [
      'import os, sys',
      'from PIL import Image',
      'out, cols, folder = sys.argv[1:]',
      'cols = int(cols)',
      'cells = sorted(os.path.join(folder, name) for name in os.listdir(folder) if name.endswith(".png"))',
      'with Image.open(cells[0]) as first:',
      '    w, h = int(first.width * 0.6), int(first.height * 0.6)',
      'rows = -(-len(cells) // cols)',
      'pad = 6',
      'sheet = Image.new("RGB", (cols * (w + pad) + pad, rows * (h + pad) + pad), (128, 128, 128))',
      'for i, cell in enumerate(cells):',
      '    with Image.open(cell) as im:',
      '        sheet.paste(im.convert("RGB").resize((w, h), Image.LANCZOS), (pad + (i % cols) * (w + pad), pad + (i // cols) * (h + pad)))',
      'sheet.save(out)',
    ].join('\n');
    execFileSync('python', ['-c', tile, file, '6', cells], { stdio: 'inherit' });
    console.log(`${file}  (frames ${frames.join(', ')})`);
  }
}

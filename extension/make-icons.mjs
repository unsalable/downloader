// The extension's toolbar icons.
//
// A sibling of the app's mark rather than a copy of it: the app draws a six-
// blade aperture, this draws the same hexagon as an outline with a dot inside,
// which is what "linked" looks like at sixteen pixels. Nothing here imitates a
// browser's or a video site's branding, which is a thing store review does look
// for in an extension that asks to read cookies.
//
// The PNG encoder is here rather than from a package because the repository has
// no image library on the JavaScript side and four flat squares are not worth
// adding one; node:zlib already does the only hard part.
//
// Run:  node extension/make-icons.mjs

import { deflateSync } from 'node:zlib';
import { mkdirSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const outDir = join(dirname(fileURLToPath(import.meta.url)), 'icons');
const SIZES = [16, 32, 48, 128];

// Supersampled and box-filtered afterwards; a hexagon drawn by testing pixel
// centres has visibly ragged diagonals at 16px otherwise.
const SAMPLES = 4;

const TILE = [0x16, 0x12, 0x0e];
const MARK = [0xff, 0x7a, 0x3d];

function insideRoundedSquare(x, y, size, radius) {
  const nearestX = Math.min(Math.max(x, radius), size - radius);
  const nearestY = Math.min(Math.max(y, radius), size - radius);
  const dx = x - nearestX;
  const dy = y - nearestY;
  return dx * dx + dy * dy <= radius * radius;
}

// Inside a regular polygon: on the inner side of all of its edges at once.
function insidePolygon(x, y, centre, radius, sides, rotation) {
  const apothem = radius * Math.cos(Math.PI / sides);
  for (let i = 0; i < sides; i += 1) {
    const angle = rotation + Math.PI / sides + (i * 2 * Math.PI) / sides;
    if ((x - centre) * Math.cos(angle) + (y - centre) * Math.sin(angle) > apothem) {
      return false;
    }
  }
  return true;
}

function sample(x, y, size) {
  if (!insideRoundedSquare(x, y, size, size * 0.22)) return null;

  const centre = size / 2;
  const radius = size * 0.365;
  // Flat edge facing up, the same orientation the application tile uses.
  const rotation = -Math.PI / 2;

  const ring =
    insidePolygon(x, y, centre, radius, 6, rotation) &&
    !insidePolygon(x, y, centre, radius * 0.63, 6, rotation);

  const dx = x - centre;
  const dy = y - centre;
  const dot = dx * dx + dy * dy <= (radius * 0.30) ** 2;

  return ring || dot ? MARK : TILE;
}

function render(size) {
  const pixels = Buffer.alloc(size * size * 4);
  const step = 1 / SAMPLES;

  for (let py = 0; py < size; py += 1) {
    for (let px = 0; px < size; px += 1) {
      let r = 0;
      let g = 0;
      let b = 0;
      let covered = 0;

      for (let sy = 0; sy < SAMPLES; sy += 1) {
        for (let sx = 0; sx < SAMPLES; sx += 1) {
          const colour = sample(px + (sx + 0.5) * step, py + (sy + 0.5) * step, size);
          if (!colour) continue;
          r += colour[0];
          g += colour[1];
          b += colour[2];
          covered += 1;
        }
      }

      const offset = (py * size + px) * 4;
      const total = SAMPLES * SAMPLES;
      if (covered === 0) continue;
      // Averaged over the covered samples, not over all of them: dividing by
      // the full count would darken every edge pixel towards black instead of
      // fading it towards transparent.
      pixels[offset] = Math.round(r / covered);
      pixels[offset + 1] = Math.round(g / covered);
      pixels[offset + 2] = Math.round(b / covered);
      pixels[offset + 3] = Math.round((covered / total) * 255);
    }
  }

  return pixels;
}

const CRC_TABLE = Array.from({ length: 256 }, (_, n) => {
  let c = n;
  for (let k = 0; k < 8; k += 1) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
  return c >>> 0;
});

function crc32(buffer) {
  let c = 0xffffffff;
  for (const byte of buffer) c = CRC_TABLE[(c ^ byte) & 0xff] ^ (c >>> 8);
  return (c ^ 0xffffffff) >>> 0;
}

function chunk(type, data) {
  const length = Buffer.alloc(4);
  length.writeUInt32BE(data.length);
  const body = Buffer.concat([Buffer.from(type, 'latin1'), data]);
  const crc = Buffer.alloc(4);
  crc.writeUInt32BE(crc32(body));
  return Buffer.concat([length, body, crc]);
}

function png(size, pixels) {
  const header = Buffer.alloc(13);
  header.writeUInt32BE(size, 0);
  header.writeUInt32BE(size, 4);
  header[8] = 8; // bit depth
  header[9] = 6; // truecolour with alpha
  // Compression 0, filter 0, interlace 0 are the only values PNG defines.

  const raw = Buffer.alloc(size * (size * 4 + 1));
  for (let y = 0; y < size; y += 1) {
    const line = y * (size * 4 + 1);
    raw[line] = 0; // no per-scanline filter; these images are tiny
    pixels.copy(raw, line + 1, y * size * 4, (y + 1) * size * 4);
  }

  return Buffer.concat([
    Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]),
    chunk('IHDR', header),
    chunk('IDAT', deflateSync(raw, { level: 9 })),
    chunk('IEND', Buffer.alloc(0)),
  ]);
}

mkdirSync(outDir, { recursive: true });
for (const size of SIZES) {
  const path = join(outDir, `icon${size}.png`);
  writeFileSync(path, png(size, render(size)));
  console.log(`make-icons: wrote ${path}`);
}

// The extension's toolbar icons.
//
// The application's own mark, not a relative of it: a six-blade aperture on
// nothing, in the same proportions scripts/generate_icon.py draws it in. It
// used to be a hexagon outline with a dot inside, which said "linked" but said
// it in a shape the product does not use anywhere else, so a user looking at
// the toolbar had no way to tell which program the button belonged to. Nothing
// here imitates a browser's or a video site's branding, which is a thing store
// review does look for in an extension that asks to read cookies.
//
// The PNG encoder is here rather than from a package because the repository has
// no image library on the JavaScript side and four small icons are not worth
// adding one; node:zlib already does the only hard part.
//
// Run:  node extension/make-icons.mjs

import { deflateSync } from 'node:zlib';
import { mkdirSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const outDir = join(dirname(fileURLToPath(import.meta.url)), 'icons');
const SIZES = [16, 32, 48, 128];

// Supersampled and box-filtered afterwards; a disc and six diagonal seams drawn
// by testing pixel centres have visibly ragged edges at 16px otherwise. Eight
// rather than four because the seams are the whole point of the mark and a
// coarse grid quantises their width into a stair.
const SAMPLES = 8;

const BLADES = 6;
// All three as fractions of the disc's diameter, matching generate_icon.py.
const MARK_FILL = 0.92;
const INNER_R = 0.335;
const SEAM_W = 0.085;

const ACCENT_A = [0xff, 0x7a, 0x3d];
const ACCENT_B = [0xff, 0x90, 0x59];

// Flat blade facing up, the same rotation the application icon uses.
const ROTATION = -Math.PI / 2;

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

// Built once per size rather than per sub-sample: the vertices and seam
// directions do not depend on the point being tested, and at 128px with eight
// samples the point being tested comes round a million times.
function apertureSampler(size) {
  const centre = size / 2;
  const diameter = size * MARK_FILL;
  const outer = diameter / 2;
  const inner = diameter * INNER_R;
  const reach = (diameter * SEAM_W) / 2;

  const vertices = [];
  for (let i = 0; i < BLADES; i += 1) {
    const angle = ROTATION + (i * 2 * Math.PI) / BLADES;
    vertices.push([centre + inner * Math.cos(angle), centre + inner * Math.sin(angle)]);
  }

  // Each seam leaves an opening vertex along that edge's direction and carries
  // on past the rim. The tangential rather than radial direction is what gives
  // an iris its characteristic swirl.
  const seams = vertices.map(([x0, y0], i) => {
    const [x1, y1] = vertices[(i + 1) % BLADES];
    const length = Math.hypot(x1 - x0, y1 - y0);
    return [x0, y0, (x1 - x0) / length, (y1 - y0) / length];
  });

  return (x, y) => {
    const dx = x - centre;
    const dy = y - centre;
    if (dx * dx + dy * dy > outer * outer) return null;
    if (insidePolygon(x, y, centre, inner, BLADES, ROTATION)) return null;

    for (const [x0, y0, ux, uy] of seams) {
      const px = x - x0;
      const py = y - y0;
      const along = px * ux + py * uy;
      // Behind the vertex the distance is measured to the vertex itself, which
      // rounds the seam off at its inner end so a blade does not finish in a
      // spike the way a squared-off cut would leave it.
      const across = along < 0 ? Math.hypot(px, py) : Math.abs(px * uy - py * ux);
      if (across <= reach) return null;
    }

    const t = (x / size) * 0.65 + (y / size) * 0.35;
    return [
      ACCENT_A[0] + (ACCENT_B[0] - ACCENT_A[0]) * t,
      ACCENT_A[1] + (ACCENT_B[1] - ACCENT_A[1]) * t,
      ACCENT_A[2] + (ACCENT_B[2] - ACCENT_A[2]) * t,
    ];
  };
}

function render(size) {
  const pixels = Buffer.alloc(size * size * 4);
  const step = 1 / SAMPLES;
  const sample = apertureSampler(size);

  for (let py = 0; py < size; py += 1) {
    for (let px = 0; px < size; px += 1) {
      let r = 0;
      let g = 0;
      let b = 0;
      let covered = 0;

      for (let sy = 0; sy < SAMPLES; sy += 1) {
        for (let sx = 0; sx < SAMPLES; sx += 1) {
          const colour = sample(px + (sx + 0.5) * step, py + (sy + 0.5) * step);
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

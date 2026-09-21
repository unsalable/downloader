// Build the Chrome Web Store upload for the connector extension.
//
// Two things make this more than `zip -r`:
//
// The `key` in extension/manifest.json pins the id of an unpacked install so
// the Rust host's allow-list can name it during development. The store issues
// its own id and Google's guidance is to strip the field before uploading, so
// it is removed from the copy that ships and left alone in the working tree.
//
// The directory also holds things that must not be published: the private half
// of that key, the icon generator, and the developer README. An allow-list of
// what belongs in a release would rot every time a file is added, so this
// excludes by name and prints what it packed -- a wrong file is then visible in
// the output rather than in the listing.
//
// The zip is written by hand because Node has no archiver and PowerShell's
// Compress-Archive has been inconsistent about path separators, which is not a
// thing worth debugging against a review queue.

import { deflateRawSync, crc32 } from 'node:zlib';
import { readdirSync, readFileSync, statSync, writeFileSync } from 'node:fs';
import { dirname, join, relative, sep } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');
const source = join(root, 'extension');
const target = join(root, 'extension-upload.zip');

const EXCLUDED = new Set([
  'key.pem',
  'make-icons.mjs',
  'README.md',
  'PRIVACY.md',
  'STORE-LISTING.md',
  '.gitignore',
]);

function collect(dir) {
  return readdirSync(dir).flatMap((name) => {
    const full = join(dir, name);
    if (EXCLUDED.has(name) || name.endsWith('.zip')) return [];
    return statSync(full).isDirectory() ? collect(full) : [full];
  });
}

function contentsOf(file) {
  if (relative(source, file) !== 'manifest.json') return readFileSync(file);

  const manifest = JSON.parse(readFileSync(file, 'utf8'));
  delete manifest.key;
  return Buffer.from(`${JSON.stringify(manifest, null, 2)}\n`, 'utf8');
}

// A fixed timestamp, so the same sources produce the same bytes and a rejected
// upload can be compared against the one before it.
const DOS_TIME = 0;
const DOS_DATE = 0x2821; // 2000-01-01

const files = collect(source).sort();
const locals = [];
const central = [];
let offset = 0;

for (const file of files) {
  const name = relative(source, file).split(sep).join('/');
  const raw = contentsOf(file);
  const body = deflateRawSync(raw, { level: 9 });
  const sum = crc32(raw);
  const named = Buffer.from(name, 'utf8');

  const local = Buffer.alloc(30);
  local.writeUInt32LE(0x04034b50, 0);
  local.writeUInt16LE(20, 4);
  local.writeUInt16LE(0, 6);
  local.writeUInt16LE(8, 8);
  local.writeUInt16LE(DOS_TIME, 10);
  local.writeUInt16LE(DOS_DATE, 12);
  local.writeUInt32LE(sum, 14);
  local.writeUInt32LE(body.length, 18);
  local.writeUInt32LE(raw.length, 22);
  local.writeUInt16LE(named.length, 26);
  locals.push(local, named, body);

  const entry = Buffer.alloc(46);
  entry.writeUInt32LE(0x02014b50, 0);
  entry.writeUInt16LE(20, 4);
  entry.writeUInt16LE(20, 6);
  entry.writeUInt16LE(0, 8);
  entry.writeUInt16LE(8, 10);
  entry.writeUInt16LE(DOS_TIME, 12);
  entry.writeUInt16LE(DOS_DATE, 14);
  entry.writeUInt32LE(sum, 16);
  entry.writeUInt32LE(body.length, 20);
  entry.writeUInt32LE(raw.length, 24);
  entry.writeUInt16LE(named.length, 28);
  entry.writeUInt32LE(offset, 42);
  central.push(entry, named);

  offset += local.length + named.length + body.length;
  console.log(`  ${name}  (${raw.length} bytes)`);
}

const directory = Buffer.concat(central);
const end = Buffer.alloc(22);
end.writeUInt32LE(0x06054b50, 0);
end.writeUInt16LE(files.length, 8);
end.writeUInt16LE(files.length, 10);
end.writeUInt32LE(directory.length, 12);
end.writeUInt32LE(offset, 16);

writeFileSync(target, Buffer.concat([...locals, directory, end]));

console.log(`\npacked ${files.length} files -> ${target}`);
console.log('the signing key and the developer notes are not in it');

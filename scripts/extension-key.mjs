// Pin the browser extension's id.
//
// An unpacked extension normally gets a new id every time it is loaded from a
// different path, and the id is what the native messaging host checks before it
// will speak to anything. Without a fixed id the host's `allowed_origins` would
// have to be rewritten on every developer's machine, so the manifest carries a
// `key` and the id follows from it.
//
// The derivation is Chrome's own: SHA-256 over the DER-encoded SubjectPublicKey-
// Info, first sixteen bytes, hex, and each hex digit shifted from 0-f onto a-p
// so the id is never all digits. It was checked against eighteen extensions
// already installed on a real Chrome profile that carry a `key` in their
// manifest; all eighteen derive back to their own directory name.
//
// Run:  node scripts/extension-key.mjs [--force]
//
// `key.pem` stays out of version control. Regenerating it changes the id, which
// means changing `EXTENSION_ID_DEV` in src-tauri/src/bridge/protocol.rs and
// re-registering the host -- so this refuses to overwrite an existing key and
// prints the id it already implies instead.

import { createHash, createPublicKey, generateKeyPairSync } from 'node:crypto';
import { existsSync, readFileSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');
const keyPath = join(root, 'extension', 'key.pem');
const manifestPath = join(root, 'extension', 'manifest.json');
const force = process.argv.includes('--force');

/** The manifest `key`: the public half, SPKI DER, base64. */
function manifestKey(privatePem) {
  const der = createPublicKey(privatePem).export({ type: 'spki', format: 'der' });
  return der.toString('base64');
}

function extensionId(base64Key) {
  const digest = createHash('sha256').update(Buffer.from(base64Key, 'base64')).digest('hex');
  return [...digest.slice(0, 32)]
    .map((hex) => String.fromCharCode(parseInt(hex, 16) + 'a'.charCodeAt(0)))
    .join('');
}

if (existsSync(keyPath) && !force) {
  const key = manifestKey(readFileSync(keyPath, 'utf8'));
  console.log('extension-key: key.pem already exists, leaving it alone');
  console.log(`  key: ${key}`);
  console.log(`  id:  ${extensionId(key)}`);
  process.exit(0);
}

// 2048 bits because that is what Chrome's own packer produces and what every
// published extension carries; a larger key only makes the manifest longer.
const { privateKey } = generateKeyPairSync('rsa', { modulusLength: 2048 });
const pem = privateKey.export({ type: 'pkcs8', format: 'pem' });
writeFileSync(keyPath, pem, { mode: 0o600 });

const key = manifestKey(pem);
const id = extensionId(key);

const manifest = JSON.parse(readFileSync(manifestPath, 'utf8'));
manifest.key = key;
writeFileSync(manifestPath, `${JSON.stringify(manifest, null, 2)}\n`);

console.log(`extension-key: wrote ${keyPath}`);
console.log(`extension-key: wrote the key into ${manifestPath}`);
console.log(`  key: ${key}`);
console.log(`  id:  ${id}`);
console.log('');
console.log('Set EXTENSION_ID_DEV in src-tauri/src/bridge/protocol.rs to that id.');

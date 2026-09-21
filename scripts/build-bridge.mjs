// Build the browser link's native messaging host and stage it where the
// bundler expects it.
//
// `ud-bridge.exe` is a second binary in the same crate, so cargo already builds
// it alongside the app; this only puts a copy at a fixed path, because a Tauri
// resource entry has to name a file that exists before the bundler runs and
// `target/debug` and `target/release` are not the same path.
//
// Windows only, and silent everywhere else: the browser link does not exist on
// other platforms, and `npm run app:build` on one should not fail over it.

import { execFileSync } from 'node:child_process';
import { copyFileSync, existsSync, mkdirSync, rmSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');
const release = process.argv.includes('--release');

if (process.platform !== 'win32') {
  console.log('build-bridge: not Windows, nothing to do');
  process.exit(0);
}

const staged = join(root, 'src-tauri', 'binaries', 'ud-bridge.exe');
const built = join(root, 'src-tauri', 'target', release ? 'release' : 'debug', 'ud-bridge.exe');

// Tauri's build script checks that every bundle resource exists before cargo
// compiles anything, and the resource in question is a binary from this same
// crate -- so on a clean checkout the check fails on the file the build is
// about to produce. An empty placeholder breaks that circle; the real binary
// lands on top of it a moment later, and a failed build takes the placeholder
// with it so a later `tauri build` can never bundle a zero-byte helper.
mkdirSync(dirname(staged), { recursive: true });
const bootstrapped = !existsSync(staged);
if (bootstrapped) writeFileSync(staged, '');

const args = ['build', '--manifest-path', join(root, 'src-tauri', 'Cargo.toml'), '--bin', 'ud-bridge'];
if (release) args.push('--release');

try {
  execFileSync('cargo', args, { stdio: 'inherit' });
} catch (error) {
  if (bootstrapped) rmSync(staged, { force: true });
  throw error;
}

copyFileSync(built, staged);
console.log(`build-bridge: staged ${staged}`);

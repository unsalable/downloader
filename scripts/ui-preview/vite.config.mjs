/**
 * Development helper: the desktop or phone interface in a plain browser.
 *
 * Without a Tauri backend `getSettings()` rejects, the settings gate never
 * opens and the app paints an empty background forever. This config serves the
 * real frontend with `mock-tauri.js` injected ahead of the module script, so
 * every screen can be looked at -- and driven -- without building the Rust side.
 *
 *   npx vite --config scripts/ui-preview/vite.config.mjs
 *
 * then open http://localhost:1430/ (desktop) or /?platform=android (phone).
 * See the header of mock-tauri.js for the other switches.
 */
import { readFileSync } from 'node:fs';
import { fileURLToPath, URL } from 'node:url';

import tailwindcss from '@tailwindcss/vite';
import react from '@vitejs/plugin-react';
import { defineConfig } from 'vite';

const root = fileURLToPath(new URL('../..', import.meta.url));
const mock = fileURLToPath(new URL('./mock-tauri.js', import.meta.url));

export default defineConfig({
  root,
  plugins: [
    react(),
    tailwindcss(),
    {
      name: 'ud-mock-tauri',
      transformIndexHtml() {
        // Inline and first in <head>: the app reads the platform while its
        // modules are still being evaluated.
        return [{ tag: 'script', children: readFileSync(mock, 'utf8'), injectTo: 'head-prepend' }];
      },
    },
  ],
  resolve: {
    alias: { '@': fileURLToPath(new URL('../../src', import.meta.url)) },
  },
  clearScreen: false,
  server: {
    port: 1430,
    strictPort: true,
    // A Rust build drops .pdb files the watcher chokes on.
    watch: { ignored: ['**/src-tauri/**'] },
  },
});

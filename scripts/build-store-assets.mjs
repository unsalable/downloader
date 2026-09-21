// Produce the Chrome Web Store listing images from the real popup.
//
// The popup is rendered for real -- its own HTML, CSS and JavaScript, with the
// handful of chrome.* calls it makes standing in -- inside a frame the exact
// size the store wants, and photographed with headless Chrome. Nothing here is
// a mock-up of the interface; it is the interface.
//
// Throwaway tooling: it writes to store-assets/ and keeps nothing else.

import { execFileSync } from 'node:child_process';
import { mkdirSync, readFileSync, writeFileSync, existsSync, rmSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const repo = join(dirname(fileURLToPath(import.meta.url)), '..');
const work = join(repo, '.store-build');
const outRoot = join(repo, 'store-assets');
const CHROME = 'C:/Program Files/Google/Chrome/Application/chrome.exe';

const HOST = String.raw`C:\Program Files\Universal Downloader\ud-bridge.exe`;
const now = Math.floor(Date.now() / 1000);
const base = { appVersion: '1.0.0', hostPath: HOST, enabled: true };

const REPLIES = {
  connected: {
    signedIn: true,
    result: { ok: true, status: { ...base, bound: true, session: 'fresh', accountHint: 'm•••@gmail.com', lastPushAt: now - 120 } },
  },
  unpaired: {
    signedIn: true,
    result: { ok: true, status: { ...base, bound: false, session: 'none' } },
  },
};

const COPY = {
  en: {
    connected: ['Your YouTube sign-in, lent to your own computer', 'The app can now download what your membership already gives you access to.'],
    unpaired: ['One button, and it is connected', 'No codes to copy, no files to edit, no timer to beat.'],
    details: ['Nothing leaves your machine', 'No server, no analytics, no account. The session goes to the app on your own computer and nowhere else.'],
    tile: ['Universal Downloader', 'Connector'],
  },
  tr: {
    connected: ['YouTube oturumunuz, kendi bilgisayarınıza ödünç', 'Uygulama artık üyeliğinizin zaten erişim verdiği içeriği indirebilir.'],
    unpaired: ['Tek düğme, bağlantı kuruldu', 'Kopyalanacak kod, düzenlenecek dosya, yetişilecek sayaç yok.'],
    details: ['Hiçbir şey bilgisayarınızdan çıkmaz', 'Sunucu yok, analitik yok, hesap yok. Oturum yalnızca kendi bilgisayarınızdaki uygulamaya gider.'],
    tile: ['Universal Downloader', 'Connector'],
  },
};

// ---------------------------------------------------------------- build pages

if (existsSync(work)) rmSync(work, { recursive: true, force: true });
mkdirSync(work, { recursive: true });

for (const file of ['popup.html', 'popup.css', 'theme.css', 'popup.js', 'i18n.js']) {
  writeFileSync(join(work, file), readFileSync(join(repo, 'extension', file)));
}

const shots = [];

for (const locale of ['en', 'tr']) {
  const raw = JSON.parse(readFileSync(join(repo, `extension/_locales/${locale}/messages.json`), 'utf8'));

  for (const [state, reply] of Object.entries(REPLIES)) {
    // The whole message entry, placeholders included, rather than just the
    // string: "$COUNT$ dakika once" reaching a listing image unsubstituted is
    // the kind of detail that makes a product look unfinished, and it is the
    // substitution that has to be stood in for, not the lookup.
    const mock = `const ENTRIES = ${JSON.stringify(raw)};
const REPLY = ${JSON.stringify(reply)};
window.chrome = {
  i18n: {
    getMessage: (key, subs) => {
      const entry = ENTRIES[key];
      if (!entry) return '';
      const list = subs == null ? [] : (Array.isArray(subs) ? subs : [subs]);
      let text = entry.message;
      for (const [name, def] of Object.entries(entry.placeholders || {})) {
        const index = Number(String(def.content).replace('$', '')) - 1;
        text = text.replace(new RegExp('\\\\$' + name + '\\\\$', 'gi'), list[index] ?? '');
      }
      return text;
    },
    getUILanguage: () => '${locale}',
  },
  runtime: { getManifest: () => ({ version: '1.0.1' }), sendMessage: async () => REPLY },
  permissions: { contains: async () => false, request: async () => true },
};`;
    writeFileSync(join(work, `mock-${locale}-${state}.js`), mock);

    // Two things the real popup gets from its surroundings and a photograph
    // has to be given. The dark palette, because headless Chrome reports a
    // light preference and the app itself defaults to dark, so a light popup
    // would be the odd one out beside its own window. And stillness: the
    // entrance animation is caught part-way through by a screenshot, which is
    // how a title ends up half-faded. Both are states the popup genuinely has
    // -- a user with a dark browser and reduced motion sees exactly this.
    const forScreenshot = `<style>
      :root {
        --bg:#0c0b09; --surface:#15130f; --surface-sunken:#100e0b;
        --border:rgb(255 244 228 / 0.09); --border-strong:rgb(255 244 228 / 0.18);
        --text-primary:#f4efe5; --text-secondary:#a69c8c; --text-tertiary:#8b8173;
        --accent:#ff7a3d; --accent-hover:#ff9059; --accent-fg:#1a0c04;
        --accent-soft:rgb(255 122 61 / 0.14); --accent-ring:rgb(255 122 61 / 0.4);
        --success:#58cd8e; --warning:#efc059; --error:#ff6f5e;
      }
      html, body { width: 360px; margin: 0; }
      *, *::before, *::after { animation: none !important; transition: none !important; }
    </style>`;

    const page = readFileSync(join(repo, 'extension/popup.html'), 'utf8')
      .replace('<script src="i18n.js"></script>', `<script src="mock-${locale}-${state}.js"></script>\n<script src="i18n.js"></script>`)
      .replace('</head>', `${forScreenshot}</head>`);
    writeFileSync(join(work, `popup-${locale}-${state}.html`), page);
  }

  const frames = [
    { name: '1-connected', state: 'connected', copy: COPY[locale].connected, open: false },
    { name: '2-connect', state: 'unpaired', copy: COPY[locale].unpaired, open: false },
    { name: '3-privacy', state: 'connected', copy: COPY[locale].details, open: true },
  ];

  for (const frame of frames) {
    const html = `<!doctype html><html lang="${locale}"><head><meta charset="utf-8">
<style>
  :root { color-scheme: dark; }
  html, body { margin: 0; width: 1280px; height: 800px; overflow: hidden; }
  body {
    background: radial-gradient(120% 90% at 18% 8%, #241a13 0%, #16110d 55%, #100c09 100%);
    font-family: ui-sans-serif, system-ui, "Segoe UI", Roboto, sans-serif;
    display: grid; grid-template-columns: 1fr 440px; align-items: center;
    padding: 0 84px; box-sizing: border-box; gap: 48px;
  }
  h1 { color: #f6efe6; font-size: 46px; line-height: 1.15; letter-spacing: -0.02em; margin: 0 0 20px; font-weight: 600; max-width: 15ch; }
  p { color: #b3a695; font-size: 21px; line-height: 1.5; margin: 0; max-width: 34ch; }
  .brand { display: flex; align-items: center; gap: 10px; margin-bottom: 30px; color: #bd4a0c; font-size: 15px; letter-spacing: 0.10em; text-transform: uppercase; font-weight: 600; }
  .brand svg { width: 20px; height: 20px; }
  .shot { justify-self: center; width: 360px; border-radius: 14px; overflow: hidden;
          box-shadow: 0 40px 90px rgb(0 0 0 / 0.55), 0 0 0 1px rgb(255 255 255 / 0.07); }
  iframe { width: 360px; height: 420px; border: 0; display: block; background: #0c0b09; }
</style></head><body>
<div>
  <div class="brand">
    <svg viewBox="0 0 24 24"><path d="M12 2.5 20.2 7.25v9.5L12 21.5 3.8 16.75v-9.5Z" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linejoin="round"/></svg>
    <span>Universal Downloader</span>
  </div>
  <h1>${frame.copy[0]}</h1>
  <p>${frame.copy[1]}</p>
</div>
<div class="shot"><iframe id="f" src="popup-${locale}-${frame.state}.html"></iframe></div>
<script>
  // The frame is sized to whatever the popup turns out to be, rather than to a
  // guess: a fixed height leaves an empty band under the shorter states, and
  // that band is the first thing the eye lands on in a listing image.
  const frame = document.getElementById('f');
  frame.addEventListener('load', () => {
    const d = frame.contentDocument;
    ${frame.open ? "d.getElementById('detailsHead')?.click();" : ''}
    // Measured from where the content actually stops, not from scrollHeight:
    // the document keeps reporting the height the frame gives it, so asking it
    // would just hand back the guess it was seeded with.
    setTimeout(() => {
      const bottom = [...d.body.children]
        .filter((node) => !node.hidden)
        .reduce((low, node) => Math.max(low, node.getBoundingClientRect().bottom), 0);
      frame.style.height = Math.ceil(bottom) + 'px';
    }, 250);
  });
</script>
</body></html>`;
    const file = `shot-${locale}-${frame.name}.html`;
    writeFileSync(join(work, file), html);
    shots.push({ file, out: join(outRoot, locale, `${frame.name}.png`), w: 1280, h: 800 });
  }

  const tile = `<!doctype html><html><head><meta charset="utf-8"><style>
  html, body { margin: 0; width: 440px; height: 280px; overflow: hidden; }
  body { background: radial-gradient(120% 120% at 20% 0%, #2a1e14 0%, #15100c 70%);
         font-family: ui-sans-serif, system-ui, "Segoe UI", Roboto, sans-serif;
         display: flex; flex-direction: column; align-items: center; justify-content: center; gap: 16px; }
  svg { width: 62px; height: 62px; color: #bd4a0c; }
  strong { color: #f6efe6; font-size: 25px; font-weight: 600; letter-spacing: -0.01em; }
  span { color: #9c8f7f; font-size: 16px; letter-spacing: 0.16em; text-transform: uppercase; }
</style></head><body>
  <svg viewBox="0 0 24 24"><path d="M12 2.5 20.2 7.25v9.5L12 21.5 3.8 16.75v-9.5Z" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linejoin="round"/></svg>
  <strong>${COPY[locale].tile[0]}</strong>
  <span>${COPY[locale].tile[1]}</span>
</body></html>`;
  writeFileSync(join(work, `tile-${locale}.html`), tile);
  shots.push({ file: `tile-${locale}.html`, out: join(outRoot, locale, 'promo-tile-440x280.png'), w: 440, h: 280 });
}

// ------------------------------------------------------------------- capture

// Served straight off disk rather than over a local HTTP server: a server is
// one more thing that can fail to bind or fail to be reached, and an iframe of
// a sibling file is exactly what --allow-file-access-from-files is for.

// A headless run must be given a profile directory of its own. Without one it
// reaches for the default profile, which the user's own Chrome already holds
// open, and waits for a lock that is never coming.
const profile = join(work, 'chrome-profile');

const only = process.argv[2];
for (const shot of shots) {
  if (only && !shot.file.includes(only)) continue;
  mkdirSync(join(shot.out, '..'), { recursive: true });
  execFileSync(CHROME, [
    '--headless=new',
    '--disable-gpu',
    '--no-first-run',
    '--no-default-browser-check',
    '--disable-extensions',
    '--allow-file-access-from-files',
    `--user-data-dir=${profile}`,
    '--hide-scrollbars',
    '--force-device-scale-factor=1',
    `--window-size=${shot.w},${shot.h}`,
    '--virtual-time-budget=4000',
    `--screenshot=${shot.out}`,
    `file:///${join(work, shot.file).replace(/\\/g, '/')}`,
  ], { stdio: 'inherit', timeout: 90000 });
  console.log(`${shot.out.replace(repo + '\\', '').replace(/\\/g, '/')}  ${shot.w}x${shot.h}`);
}

rmSync(work, { recursive: true, force: true });

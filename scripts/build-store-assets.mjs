// Produce the Chrome Web Store listing images from the real popup.
//
// The popup is rendered for real -- its own HTML, CSS and JavaScript, with the
// handful of chrome.* calls it makes standing in -- inside a frame the exact
// size the store wants, and photographed with headless Chrome. Nothing here is
// a mock-up of the interface; it is the interface. Even the rows are the real
// thing: they come out of extension/media.js, fed the requests a page playing a
// stream would have made, exactly as the service worker feeds it.
//
// Throwaway tooling: it writes to store-assets/ and keeps nothing else.
//
//   node scripts/build-store-assets.mjs            dark, as the app opens on Windows
//   STORE_THEME=light node scripts/build-store-assets.mjs
//   node scripts/build-store-assets.mjs 2-sites    only the shots whose file names match

import { spawn } from 'node:child_process';
import { createServer } from 'node:http';
import { cpSync, mkdirSync, readFileSync, writeFileSync, existsSync, rmSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

import { rows } from '../extension/media.js';

const repo = join(dirname(fileURLToPath(import.meta.url)), '..');
const extension = join(repo, 'extension');
const work = join(repo, '.store-build');
const outRoot = join(repo, 'store-assets');
const CHROME = 'C:/Program Files/Google/Chrome/Application/chrome.exe';

// The popup follows the browser's theme, so either is a true picture of it.
// Dark by default: it is what the desktop app opens in, and the listing showed
// dark before.
const THEME = process.env.STORE_THEME === 'light' ? 'light' : 'dark';
const LOOK = {
  dark: {
    scheme: 'dark',
    canvas: '#161618',
    popup: '#1c1c1e',
    brand: '#ff8a4c',
    heading: '#f5f5f7',
    body: '#b2b2b7',
    // A dark shadow on a dark page shows nothing, so the window's edge is a
    // hairline, as Chrome draws round its own popups.
    edge: '0 0 0 1px rgb(255 255 255 / 0.1), 0 30px 80px -20px rgb(0 0 0 / 0.75)',
  },
  light: {
    scheme: 'light',
    canvas: '#ededf0',
    popup: '#f5f5f7',
    brand: '#ac440b',
    heading: '#1d1d1f',
    body: '#636366',
    edge: '0 24px 60px -12px rgb(0 0 0 / 0.18), 0 2px 8px rgb(0 0 0 / 0.06)',
  },
}[THEME];

// The store shows a listing's images at half size in its carousel, where a
// popup photographed at 1x is a smudge. At 1.5x its text still reads there.
const POPUP_ZOOM = 1.5;

const { version } = JSON.parse(readFileSync(join(extension, 'manifest.json'), 'utf8'));
const UA = 'Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/141.0.0.0 Safari/537.36';

// What the host says about itself, as the popup receives it from the worker.
const status = (fields) => ({
  ok: true,
  status: {
    appVersion: '1.0.0',
    hostPath: 'C:\\Program Files\\Universal Downloader\\ud-bridge.exe',
    enabled: true,
    bound: false,
    session: 'none',
    canDownload: true,
    ...fields,
  },
});
const LINK_OFF = { result: status({}), signedIn: true, linkOff: false };
const LINK_ON = { result: status({ bound: true, session: 'fresh' }), signedIn: true, linkOff: false };

// The made-up pages the screenshots show. Titles are the page's, so they are
// given per language; the addresses are reserved example domains.
const PAGES = {
  en: { series: 'The Long Road — Episode 5', concert: 'Live at the Harbour — Full Concert' },
  tr: { series: 'Uzun Yol — 5. Bölüm', concert: 'Limanda Canlı — Konserin Tamamı' },
};

let id = 0;
const item = (url, kind, extra = {}) => ({
  id: `s${(id += 1)}`,
  url,
  kind,
  frameId: 0,
  initiator: 'https://player.example',
  isXhr: kind === 'hls' || kind === 'dash',
  contentType: '',
  size: null,
  seenOn: '',
  at: id,
  ...extra,
});

// Stills for the made-up videos, drawn here so the listing shows no one's
// footage. The popup only loads pictures over http, as it does on a real page,
// so they are served from a local server for the length of the run.
const POSTERS = {
  road: `<svg xmlns="http://www.w3.org/2000/svg" width="1280" height="720" viewBox="0 0 160 90">
  <defs>
    <linearGradient id="sky" x1="0" y1="0" x2="0" y2="1">
      <stop offset="0" stop-color="#26304f"/>
      <stop offset="0.45" stop-color="#7d4f6b"/>
      <stop offset="0.68" stop-color="#d0775a"/>
      <stop offset="0.8" stop-color="#f2ab62"/>
    </linearGradient>
  </defs>
  <rect width="160" height="90" fill="url(#sky)"/>
  <circle cx="108" cy="57" r="10" fill="#ffdcaa"/>
  <path d="M0 57 C 22 47 42 50 64 56 S 118 47 160 55 V90 H0 Z" fill="#5b3648"/>
  <path d="M0 66 C 28 59 58 62 84 64 S 136 59 160 63 V90 H0 Z" fill="#33202f"/>
  <path d="M77 63.5 H83 L128 90 H32 Z" fill="#6a4552"/>
  <path d="M79.7 66 H80.3 L80.5 68.5 H79.5 Z M79.2 72 H80.8 L81.2 76.5 H78.8 Z M78.4 81 H81.6 L82.3 88 H77.7 Z" fill="#f4c98d"/>
</svg>`,
  harbour: `<svg xmlns="http://www.w3.org/2000/svg" width="1280" height="720" viewBox="0 0 160 90">
  <defs>
    <linearGradient id="night" x1="0" y1="0" x2="0" y2="1">
      <stop offset="0" stop-color="#0b1424"/>
      <stop offset="0.62" stop-color="#1c2d4a"/>
    </linearGradient>
    <radialGradient id="glow" cx="0.5" cy="0.62" r="0.42">
      <stop offset="0" stop-color="#ffbf73" stop-opacity="0.85"/>
      <stop offset="1" stop-color="#ffbf73" stop-opacity="0"/>
    </radialGradient>
  </defs>
  <rect width="160" height="90" fill="url(#night)"/>
  <rect width="160" height="90" fill="url(#glow)"/>
  <g fill="#ffd9a1" opacity="0.16">
    <path d="M80 52 L46 0 H60 Z"/><path d="M80 52 L74 0 H88 Z"/><path d="M80 52 L104 0 H118 Z"/>
  </g>
  <rect x="54" y="50" width="52" height="7" rx="1" fill="#141b2b"/>
  <rect x="54" y="50" width="52" height="1.2" fill="#ffc985"/>
  <rect y="57" width="160" height="33" fill="#0d1829"/>
  <g fill="#ffbf73" opacity="0.4">
    <rect x="66" y="60" width="28" height="1.1"/><rect x="70" y="63.5" width="20" height="1"/><rect x="74" y="67" width="12" height="0.9"/>
  </g>
  <g fill="#060a13">
    <circle cx="14" cy="86" r="6"/><circle cx="30" cy="84" r="5.5"/><circle cx="47" cy="86" r="6"/><circle cx="64" cy="85" r="5"/>
    <circle cx="96" cy="85" r="5.5"/><circle cx="113" cy="86" r="6"/><circle cx="130" cy="84" r="5.5"/><circle cx="147" cy="86" r="6"/>
    <rect y="86" width="160" height="4"/>
  </g>
</svg>`,
};
let posterBase = '';
const poster = (name) => `${posterBase}/${name}.svg`;

function seriesRows(title) {
  const page = 'https://tv.example/series/the-long-road/5';
  const state = {
    url: page,
    title,
    sawMedia: true,
    items: [
      item('https://cdn.example/hls/the-long-road-5/master.m3u8', 'hls', {
        info: { master: true, variants: [], height: 1080, protected: false },
      }),
      item('https://cdn.example/files/the-long-road-5-trailer.mp4', 'video', { size: 48 * 1024 * 1024 }),
      item('https://cdn.example/podcast/the-long-road-commentary.mp3', 'audio', { size: 31 * 1024 * 1024 }),
    ],
  };
  const top = {
    frameId: 0,
    href: page,
    isTop: true,
    title,
    ogTitle: title,
    ogImage: '',
    ogUrl: '',
    videos: [{ src: '', poster: poster('road'), duration: 1421, width: 1920, height: 1080, drm: false }],
    audios: [],
  };
  return rows(state, [top], { userAgent: UA });
}

// A known site's page, offered whole so the app's engine reads it. Vimeo rather
// than YouTube: the store refuses listings that advertise downloading from
// YouTube, whatever the extension itself does there.
const CONCERT_PAGE = 'https://vimeo.com/storeshot';

function concertRows(title) {
  const page = CONCERT_PAGE;
  const top = {
    frameId: 0,
    href: page,
    isTop: true,
    title: `${title} on Vimeo`,
    ogTitle: title,
    ogImage: poster('harbour'),
    ogUrl: page,
    videos: [{ src: '', poster: '', duration: 5468, width: 1920, height: 1080, drm: false }],
    audios: [],
  };
  return rows({ url: page, title, sawMedia: true, items: [] }, [top], { userAgent: UA });
}

// What the app's helper says each row would download, as the popup receives
// it from the worker. The figures are the kind a real reading gives for files
// of that length; nothing in the picture is asked of a real server.
function previewsFor(list, extra) {
  const out = {};
  for (const row of list) {
    const preview = { title: row.title, durationSec: row.durationSec, ...extra[row.kind] };
    // The app's own reading of a page brings the page's still with it.
    if (preview.thumbnail === '') preview.thumbnail = row.thumbnail;
    out[row.payload.url] = { preview, protected: false };
  }
  return out;
}

const SERIES_DETAILS = {
  stream: { qualityLabel: '1080p', container: 'mp4', estimatedBytes: 1.24 * 1024 ** 3, durationSec: 1421 },
  video: { qualityLabel: '1080p', container: 'mp4', estimatedBytes: 48 * 1024 ** 2, durationSec: 154 },
  audio: { qualityLabel: '192 kbps', container: 'mp3', estimatedBytes: 31 * 1024 ** 2, durationSec: 1355, audioOnly: true },
};
const CONCERT_DETAILS = {
  page: { qualityLabel: '1080p', container: 'mkv', estimatedBytes: 2.31 * 1024 ** 3, durationSec: 5468, platform: 'Vimeo', thumbnail: '' },
};

const COPY = {
  en: {
    videos: ['Every video on the page, one button away', 'See what you will get, press Get, and Universal Downloader takes it from there.'],
    sites: ['Works on the sites you already use', 'Vimeo, X, Dailymotion and more go straight to the app at your default quality.'],
    privacy: ['Nothing leaves your computer', 'No server, no analytics, no account. The extension talks only to Universal Downloader on this computer.'],
    tile: ['Universal Downloader', 'Connector'],
  },
  tr: {
    videos: ['Sayfadaki her video, bir düğme uzağınızda', "Kaç p, hangi biçim, ne kadar yer tutacağı önceden görünür. İndir'e basın; gerisini Universal Downloader halleder."],
    sites: ['Zaten kullandığınız sitelerde çalışır', 'Vimeo, X, Dailymotion ve daha fazlası varsayılan kalitenizle doğrudan uygulamaya gider.'],
    privacy: ['Hiçbir şey bilgisayarınızdan çıkmaz', 'Sunucu yok, analitik yok, hesap yok. Eklenti yalnızca bu bilgisayardaki Universal Downloader ile konuşur.'],
    tile: ['Universal Downloader', 'Connector'],
  },
};

// ---------------------------------------------------------------- build pages

if (existsSync(work)) rmSync(work, { recursive: true, force: true });
mkdirSync(work, { recursive: true });

for (const file of ['popup.html', 'popup.css', 'theme.css', 'popup.js', 'i18n.js', 'media.js']) {
  writeFileSync(join(work, file), readFileSync(join(extension, file)));
}
cpSync(join(extension, 'fonts'), join(work, 'fonts'), { recursive: true });

const server = createServer((req, res) => {
  const name = (req.url ?? '').replace(/^\/|\.svg$/g, '');
  if (!Object.hasOwn(POSTERS, name)) {
    res.writeHead(404).end();
    return;
  }
  res.writeHead(200, { 'content-type': 'image/svg+xml' }).end(POSTERS[name]);
});
await new Promise((resolve) => server.listen(0, '127.0.0.1', resolve));
posterBase = `http://127.0.0.1:${server.address().port}`;

const shots = [];

for (const locale of ['en', 'tr']) {
  const raw = JSON.parse(readFileSync(join(extension, `_locales/${locale}/messages.json`), 'utf8'));
  const titles = PAGES[locale];

  const series = seriesRows(titles.series);
  const concert = concertRows(titles.concert);
  const states = {
    videos: { url: 'https://tv.example/series/the-long-road/5', scan: series, link: LINK_OFF, previews: previewsFor(series, SERIES_DETAILS) },
    sites: { url: CONCERT_PAGE, scan: concert, link: LINK_ON, previews: previewsFor(concert, CONCERT_DETAILS) },
  };

  for (const [state, setup] of Object.entries(states)) {
    // The whole message entry, placeholders included, rather than just the
    // string, so a message that ever grows a placeholder still renders whole.
    const mock = `const ENTRIES = ${JSON.stringify(raw)};
const SETUP = ${JSON.stringify(setup)};
const area = () => ({ get: async () => ({}), set: async () => {}, remove: async () => {} });
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
  runtime: {
    id: 'store-shot',
    getManifest: () => ({ version: '${version}' }),
    sendMessage: async (message) => {
      switch (message.action) {
        case 'scan': return { ok: true, url: SETUP.url, protectedService: null, rows: SETUP.scan };
        case 'probe': return SETUP.previews[message.payload?.url] ?? { preview: null, protected: false };
        case 'download': return { result: SETUP.link.result };
        default: return SETUP.link;
      }
    },
  },
  tabs: { query: async () => [{ id: 1, url: SETUP.url }] },
  storage: { local: area(), session: area() },
};`;
    writeFileSync(join(work, `mock-${locale}-${state}.js`), mock);

    // Stillness: the list's entrance and the switch's slide are caught part-way
    // through by a screenshot. A user with reduced motion sees exactly this.
    const forScreenshot = `<style>
      html, body { margin: 0; }
      *, *::before, *::after { animation: none !important; transition: none !important; }
    </style>`;

    const page = readFileSync(join(extension, 'popup.html'), 'utf8')
      .replace('<script src="i18n.js"></script>', `<script src="mock-${locale}-${state}.js"></script>\n<script src="i18n.js"></script>`)
      .replace('</head>', `${forScreenshot}</head>`);
    writeFileSync(join(work, `popup-${locale}-${state}.html`), page);
  }

  const frames = [
    { name: '1-videos', state: 'videos', copy: COPY[locale].videos, press: false },
    { name: '2-sites', state: 'sites', copy: COPY[locale].sites, press: false },
    { name: '3-privacy', state: 'videos', copy: COPY[locale].privacy, press: true },
  ];

  for (const frame of frames) {
    // The app's palette, flat: the user's taste rules out decorative
    // gradients, and the popup is the only thing in the picture that floats.
    // The iframe's colour scheme is the page's, so the popup inside it takes
    // its own dark or light tokens exactly as it does in the browser.
    const html = `<!doctype html><html lang="${locale}"><head><meta charset="utf-8">
<style>
  @font-face { font-family: "InterVariable"; font-weight: 100 900; src: url("fonts/InterVariable.woff2") format("woff2"); }
  :root { color-scheme: ${LOOK.scheme}; }
  html, body { margin: 0; width: 1280px; height: 800px; overflow: hidden; }
  body {
    background: ${LOOK.canvas};
    font-family: "InterVariable", -apple-system, "Segoe UI Variable Text", "Segoe UI", system-ui, sans-serif;
    -webkit-font-smoothing: antialiased;
    display: grid; grid-template-columns: 1fr ${Math.round(340 * POPUP_ZOOM)}px; align-items: center;
    padding: 0 88px; box-sizing: border-box; gap: 64px;
  }
  .brand { color: ${LOOK.brand}; font-size: 16px; font-weight: 600; margin-bottom: 22px; }
  h1 { color: ${LOOK.heading}; font-size: 46px; line-height: 1.12; letter-spacing: -0.025em; margin: 0 0 20px; font-weight: 650; max-width: 14ch; }
  p { color: ${LOOK.body}; font-size: 20px; line-height: 1.5; margin: 0; max-width: 30ch; }
  .shot { justify-self: center; width: 340px; zoom: ${POPUP_ZOOM}; border-radius: 12px; overflow: hidden; box-shadow: ${LOOK.edge}; }
  iframe { width: 340px; height: 420px; border: 0; display: block; background: ${LOOK.popup}; }
</style></head><body>
<div>
  <div class="brand">Universal Downloader</div>
  <h1>${frame.copy[0]}</h1>
  <p>${frame.copy[1]}</p>
</div>
<div class="shot"><iframe id="f" src="popup-${locale}-${frame.state}.html"></iframe></div>
<script>
  // The frame is sized to whatever the popup turns out to be, rather than to a
  // guess: a fixed height leaves an empty band under the shorter states.
  const frame = document.getElementById('f');
  frame.addEventListener('load', () => {
    const d = frame.contentDocument;
    ${frame.press ? "setTimeout(() => d.querySelector('.get')?.click(), 200);" : ''}
    // Measured from where the content actually stops, not from scrollHeight:
    // the document keeps reporting the height the frame gives it.
    setTimeout(() => {
      const style = getComputedStyle(d.body);
      const bottom = [...d.body.children]
        .filter((node) => !node.hidden && node.tagName !== 'SCRIPT')
        .reduce((low, node) => Math.max(low, node.getBoundingClientRect().bottom), 0);
      frame.style.height = Math.ceil(bottom + parseFloat(style.paddingBottom)) + 'px';
    }, 900);
  });
</script>
</body></html>`;
    const file = `shot-${locale}-${frame.name}.html`;
    writeFileSync(join(work, file), html);
    shots.push({ file, out: join(outRoot, locale, `${frame.name}.png`), w: 1280, h: 800 });
  }

  const tile = `<!doctype html><html><head><meta charset="utf-8"><style>
  @font-face { font-family: "InterVariable"; font-weight: 100 900; src: url("fonts/InterVariable.woff2") format("woff2"); }
  html, body { margin: 0; width: 440px; height: 280px; overflow: hidden; }
  body { background: ${LOOK.popup}; font-family: "InterVariable", -apple-system, "Segoe UI", system-ui, sans-serif;
         -webkit-font-smoothing: antialiased;
         display: flex; flex-direction: column; align-items: center; justify-content: center; gap: 4px; }
  img { width: 64px; height: 64px; margin-bottom: 14px; }
  strong { color: ${LOOK.heading}; font-size: 25px; font-weight: 650; letter-spacing: -0.02em; }
  span { color: ${LOOK.body}; font-size: 17px; }
</style></head><body>
  <img src="icon128.png" alt="">
  <strong>${COPY[locale].tile[0]}</strong>
  <span>${COPY[locale].tile[1]}</span>
</body></html>`;
  writeFileSync(join(work, `tile-${locale}.html`), tile);
  shots.push({ file: `tile-${locale}.html`, out: join(outRoot, locale, 'promo-tile-440x280.png'), w: 440, h: 280 });
}

// The tile shows the toolbar icon itself, the one Chrome shows next to the
// address bar, rather than a second drawing of the mark.
writeFileSync(join(work, 'icon128.png'), readFileSync(join(extension, 'icons', 'icon128.png')));

// ------------------------------------------------------------------- capture

// Served straight off disk rather than over a local HTTP server: a server is
// one more thing that can fail to bind or fail to be reached, and an iframe of
// a sibling file -- and the module scripts inside it -- is exactly what
// --allow-file-access-from-files is for.

// A headless run must be given a profile directory of its own. Without one it
// reaches for the default profile, which the user's own Chrome already holds
// open, and waits for a lock that is never coming.
const profile = join(work, 'chrome-profile');

// Run without blocking: the poster server answers from this same process.
function capture(args) {
  return new Promise((resolve, reject) => {
    const child = spawn(CHROME, args, { stdio: 'inherit' });
    const timer = setTimeout(() => child.kill(), 90000);
    child.on('error', reject);
    child.on('close', (code) => {
      clearTimeout(timer);
      if (code === 0) resolve();
      else reject(new Error(`chrome exited with ${code}`));
    });
  });
}

const only = process.argv[2];
for (const shot of shots) {
  if (only && !shot.file.includes(only)) continue;
  mkdirSync(join(shot.out, '..'), { recursive: true });
  await capture([
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
  ]);
  console.log(`${shot.out.replace(repo + '\\', '').replace(/\\/g, '/')}  ${shot.w}x${shot.h}`);
}

server.close();
rmSync(work, { recursive: true, force: true });

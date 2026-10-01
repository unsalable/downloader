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

import { execFileSync } from 'node:child_process';
import { cpSync, mkdirSync, readFileSync, writeFileSync, existsSync, rmSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

import { rows } from '../extension/media.js';

const repo = join(dirname(fileURLToPath(import.meta.url)), '..');
const extension = join(repo, 'extension');
const work = join(repo, '.store-build');
const outRoot = join(repo, 'store-assets');
const CHROME = 'C:/Program Files/Google/Chrome/Application/chrome.exe';

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
    videos: [{ src: '', poster: '', duration: 1421, width: 1920, height: 1080, drm: false }],
    audios: [],
  };
  return rows(state, [top], { userAgent: UA });
}

function concertRows(title) {
  const page = 'https://www.youtube.com/watch?v=StoreShot01';
  const top = {
    frameId: 0,
    href: page,
    isTop: true,
    title: `${title} - YouTube`,
    ogTitle: '',
    ogImage: '',
    ogUrl: '',
    videos: [{ src: '', poster: '', duration: 5468, width: 1920, height: 1080, drm: false }],
    audios: [],
  };
  return rows({ url: page, title, sawMedia: true, items: [] }, [top], { userAgent: UA });
}

// Thumbnails stay out of the photographs: they would have to be fetched from
// the network mid-shot, and a listing image that depends on a server
// answering is not reproducible. The popup draws its own tile without one.
const still = (list) => list.map((row) => ({ ...row, thumbnail: '' }));

const COPY = {
  en: {
    videos: ['Every video on the page, one button away', 'Play it, press Get, and Universal Downloader takes it from there.'],
    sites: ['Works on the sites you already use', 'YouTube, Vimeo, X and more go straight to the app at your default quality.'],
    privacy: ['Nothing leaves your computer', 'No server, no analytics, no account. A video goes to the app on this computer, and only when you press Get.'],
    tile: ['Universal Downloader', 'Connector'],
  },
  tr: {
    videos: ['Sayfadaki her video, bir düğme uzağınızda', "Oynatın, İndir'e basın; gerisini Universal Downloader halleder."],
    sites: ['Zaten kullandığınız sitelerde çalışır', 'YouTube, Vimeo, X ve daha fazlası varsayılan kalitenizle doğrudan uygulamaya gider.'],
    privacy: ['Hiçbir şey bilgisayarınızdan çıkmaz', "Sunucu yok, analitik yok, hesap yok. Video yalnızca bu bilgisayardaki uygulamaya ve yalnızca İndir'e bastığınızda gider."],
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

const shots = [];

for (const locale of ['en', 'tr']) {
  const raw = JSON.parse(readFileSync(join(extension, `_locales/${locale}/messages.json`), 'utf8'));
  const titles = PAGES[locale];

  const states = {
    videos: { url: 'https://tv.example/series/the-long-road/5', scan: still(seriesRows(titles.series)), link: LINK_OFF },
    sites: { url: 'https://www.youtube.com/watch?v=StoreShot01', scan: still(concertRows(titles.concert)), link: LINK_ON },
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
    // The app's light palette, flat: the user's taste rules out decorative
    // gradients, and the popup is the only thing in the picture that floats.
    const html = `<!doctype html><html lang="${locale}"><head><meta charset="utf-8">
<style>
  @font-face { font-family: "InterVariable"; font-weight: 100 900; src: url("fonts/InterVariable.woff2") format("woff2"); }
  :root { color-scheme: light; }
  html, body { margin: 0; width: 1280px; height: 800px; overflow: hidden; }
  body {
    background: #ededf0;
    font-family: "InterVariable", -apple-system, "Segoe UI Variable Text", "Segoe UI", system-ui, sans-serif;
    -webkit-font-smoothing: antialiased;
    display: grid; grid-template-columns: 1fr 420px; align-items: center;
    padding: 0 96px; box-sizing: border-box; gap: 64px;
  }
  .brand { color: #ac440b; font-size: 16px; font-weight: 600; margin-bottom: 22px; }
  h1 { color: #1d1d1f; font-size: 46px; line-height: 1.12; letter-spacing: -0.025em; margin: 0 0 20px; font-weight: 650; max-width: 15ch; }
  p { color: #636366; font-size: 21px; line-height: 1.5; margin: 0; max-width: 32ch; }
  .shot { justify-self: center; width: 340px; border-radius: 12px; overflow: hidden;
          box-shadow: 0 24px 60px -12px rgb(0 0 0 / 0.18), 0 2px 8px rgb(0 0 0 / 0.06); }
  iframe { width: 340px; height: 420px; border: 0; display: block; background: #f5f5f7; }
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
  body { background: #f5f5f7; font-family: "InterVariable", -apple-system, "Segoe UI", system-ui, sans-serif;
         -webkit-font-smoothing: antialiased;
         display: flex; flex-direction: column; align-items: center; justify-content: center; gap: 4px; }
  img { width: 64px; height: 64px; margin-bottom: 14px; }
  strong { color: #1d1d1f; font-size: 25px; font-weight: 650; letter-spacing: -0.02em; }
  span { color: #636366; font-size: 17px; }
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

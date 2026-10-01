// The browser half of Universal Downloader. Two jobs, one channel:
//
// - It notices the videos and sounds the open tabs play, so the popup can list
//   them, and hands the one the user picks to the app.
// - It lends the app this profile's YouTube session, when the user has turned
//   that on, so members-only videos the user pays for can be downloaded.
//
// Everything that leaves the browser goes through one-shot `sendNativeMessage`
// to the app's own helper on this computer, rather than a long-lived
// `connectNative` port. A port would be the obvious choice for a connection
// that lives as long as the browser, but an MV3 service worker does not live
// that long -- Chrome tears it down after about thirty seconds idle and takes
// the port with it. A one-shot send starts the host, says one thing, reads one
// answer and is done, and the worker may be collected the instant it returns.
// Nothing here talks to any server.
//
// The same short life is why what the worker learns about a tab is kept in
// `chrome.storage.session` rather than in a variable, and why every listener
// is registered synchronously at the top of this file: Chrome only delivers an
// event that wakes the worker to a listener that exists by the time the
// script's first run ends.
//
// The wire format is src-tauri/src/bridge/protocol.rs. Field names come from
// there, not from taste.

import {
  MAX_ITEMS,
  classify,
  cleanTitle,
  dedupeKey,
  headerMap,
  isHttpUrl,
  normaliseFrame,
  parseDash,
  parseHls,
  protectedService,
  rows,
  samePage,
  sizeFrom,
  toPayload,
  tooSmall,
} from './media.js';

const HOST_NAME = 'com.universaldownloader.bridge';
const WIRE_VERSION = 1;

// Long enough that signing in -- which writes a dozen cookies in a burst --
// produces one push rather than a dozen, short enough that the app is usable by
// the time the user has switched windows.
const PUSH_DEBOUNCE_MS = 3000;

// How often a profile the app has switched off asks whether it has been
// switched back on, when all that prompted it is a cookie change. YouTube
// rewrites a cookie on most page loads, and every ask starts the helper.
const DISABLED_ASK_MS = 10 * 60 * 1000;

const REFRESH_ALARM = 'refresh';
const OFF_KEY = 'linkOff';
const LINK_KEY = 'link';
const TAB_PREFIX = 'tab:';

// How long the popup is kept waiting on the page. A tab showing a dialog does
// not run injected scripts until the dialog is dismissed, and a popup that sits
// blank until then is worse than one that lists what the requests showed.
const COLLECT_TIMEOUT_MS = 1500;
const MANIFEST_TIMEOUT_MS = 2500;
const MANIFEST_MAX_BYTES = 256 * 1024;
const MANIFESTS_PER_SCAN = 16;

// A jar can be full of cookies and still belong to a signed-out browser:
// YouTube sets visitor and preference cookies for everyone. These are the ones
// that carry a sign-in, so their presence is what `signedIn` means. YouTube
// keeps its own copies of the SID family on youtube.com, which is the only
// domain this extension reads.
const SESSION_COOKIE_NAMES = new Set(['SID', '__Secure-1PSID', '__Secure-3PSID', 'LOGIN_INFO']);

// Cached as a promise rather than a value: two events can arrive before the
// first read of storage resolves, and two callers each minting a UUID would
// hand the app two identities for one profile, which reaches the user as a
// connection that keeps unbinding itself.
let profileIdPromise = null;

function profileId() {
  profileIdPromise ??= (async () => {
    const stored = await chrome.storage.local.get('profileId');
    if (typeof stored.profileId === 'string' && stored.profileId !== '') {
      return stored.profileId;
    }
    const minted = crypto.randomUUID();
    await chrome.storage.local.set({ profileId: minted });
    return minted;
  })();
  return profileIdPromise;
}

async function browserFamily() {
  const agent = navigator.userAgent;
  if (/\bEdg\//.test(agent)) return 'edge';
  if (/\bOPR\//.test(agent)) return 'opera';
  if (/\bVivaldi\//.test(agent)) return 'vivaldi';
  // Brave reports itself as Chrome and is only distinguishable by asking, so
  // this has to come before the Chrome test rather than after it.
  try {
    if (navigator.brave && (await navigator.brave.isBrave())) return 'brave';
  } catch {
    // An older or stricter build without the hook; Chrome is the right guess.
  }
  if (/\bChrome\//.test(agent)) return 'chrome';
  if (/\bChromium\//.test(agent)) return 'chromium';
  return 'unknown';
}

// The `Peer` every request carries. `profileLabel` is deliberately absent:
// Chrome gives an extension no way to read the profile's display name, so the
// app names the browser alone rather than inventing a label.
async function peer() {
  return {
    v: WIRE_VERSION,
    profileId: await profileId(),
    browser: await browserFamily(),
    extensionVersion: chrome.runtime.getManifest().version,
  };
}

// Chrome reports "no app" and "the app refused you" as plain English in
// `lastError`, with nothing machine-readable to switch on. Translating the two
// that have different advice attached, here and once, keeps that string out of
// the popup -- which is the only place the user would otherwise meet it.
function transportFailure(text) {
  const message = text ?? '';
  if (/not found/i.test(message)) return { ok: false, code: 'notInstalled', message };
  if (/forbidden/i.test(message)) return { ok: false, code: 'forbidden', message };
  return { ok: false, code: 'unreachable', message };
}

// Every answer comes back in one of two shapes, `{ ok: true, status }` or
// `{ ok: false, code, message }`, whatever went wrong and wherever.
function sendNative(request) {
  return new Promise((resolve) => {
    try {
      chrome.runtime.sendNativeMessage(HOST_NAME, request, (response) => {
        const failure = chrome.runtime.lastError;
        if (failure) {
          resolve(transportFailure(failure.message));
          return;
        }
        if (!response || typeof response !== 'object') {
          resolve({ ok: false, code: 'unreachable', message: '' });
          return;
        }
        if (response.ok === true) {
          resolve({ ok: true, status: response.status ?? null, preview: response.preview ?? null });
          return;
        }
        resolve({
          ok: false,
          code: response.error?.code ?? 'internal',
          message: response.error?.message ?? '',
        });
      });
    } catch (error) {
      resolve(transportFailure(String(error)));
    }
  });
}

// What the host last said about this profile, kept so the pushes nobody
// pressed a button for know whether one is wanted:
//
// - 'bound': this profile holds the binding and the app's switch is on. The
//   only state a push is for.
// - 'disabled': it holds the binding, but the switch in the app is off. The
//   host refuses pushes and the popup draws the switch off, so no cookies are
//   sent; but the app can be switched back on without this extension hearing
//   of it, so it is asked again, with a status request that carries none.
// - 'unbound': another profile holds the binding, or none does. Only Claim
//   binds, and Claim is the popup's switch, so there is nothing to ask until
//   someone presses it.
//
// Without this a profile that never turned the session on would launch the
// helper after every YouTube cookie change, only to be refused each time.
function linkOf(result) {
  if (result.ok && typeof result.status?.bound === 'boolean') {
    if (!result.status.bound) return 'unbound';
    return result.status.enabled === true ? 'bound' : 'disabled';
  }
  if (!result.ok && result.code === 'unpaired') return 'unbound';
  // Refused for the app's switch. Pushes are refused that way before the host
  // looks at the binding, so a profile that turns out not to hold it is told
  // so by the status request this leads to, and stops asking then.
  if (!result.ok && result.code === 'disabled') return 'disabled';
  return undefined;
}

async function callHost(request) {
  const result = await sendNative(request);
  const link = linkOf(result);
  if (link !== undefined) {
    try {
      await chrome.storage.local.set({ [LINK_KEY]: link });
    } catch {
      // Storage being unavailable is not a reason to drop the answer we have.
    }
  }
  return result;
}

// Whether the user turned the session off from the popup.
//
// The app forgetting the session is not enough on its own: this extension
// would keep offering one on every cookie change, and an off switch the user
// has to keep pressing is not an off switch. Kept here rather than inferred
// from the host's answers so it also holds while the app is closed.
async function isOff() {
  const stored = await chrome.storage.local.get(OFF_KEY);
  return stored[OFF_KEY] === true;
}

function setOff(off) {
  return chrome.storage.local.set({ [OFF_KEY]: off });
}

function wireCookie(cookie) {
  const out = {
    domain: cookie.domain,
    name: cookie.name,
    value: cookie.value,
    path: cookie.path,
    secure: cookie.secure,
    httpOnly: cookie.httpOnly,
  };
  // Absent means a session cookie, which the Netscape format the app writes
  // records with an expiry of zero. Sending null would say something different.
  if (typeof cookie.expirationDate === 'number') {
    out.expirationDate = cookie.expirationDate;
  }
  return out;
}

// youtube.com and nothing else. The extension has access to every site so it
// can see what tabs play, but the session it lends is YouTube's alone.
async function readJar() {
  const jar = await chrome.cookies.getAll({ domain: 'youtube.com' });
  const cookies = jar.map(wireCookie);
  const signedIn = cookies.some(
    (cookie) => SESSION_COOKIE_NAMES.has(cookie.name) && cookie.value !== '',
  );
  return { cookies, signedIn };
}

async function push() {
  const jar = await readJar();
  return callHost({
    type: 'push',
    ...(await peer()),
    signedIn: jar.signedIn,
    capturedAt: Math.floor(Date.now() / 1000),
    cookies: jar.cookies,
  });
}

let disabledAskedAt = 0;

// The pushes nobody pressed a button for: a cookie change, the daily alarm,
// the browser starting. Each needs the switch on -- this profile bound, not
// turned off here and not turned off in the app -- because cookies leave the
// browser only while it is. A profile whose state is not known for certain
// (one connected under 1.0.2, or one the app has switched off) is asked first,
// with a status request that carries no cookies.
//
// The first push after the app is switched off cannot be avoided: nothing
// tells this extension until the host refuses it, which it does without
// keeping anything. Every push after that is held back here.
async function backgroundPush({ fromCookie = false } = {}) {
  if (await isOff()) return;
  const link = (await chrome.storage.local.get(LINK_KEY))[LINK_KEY];
  if (link === 'unbound') return;
  if (link !== 'bound') {
    if (link === 'disabled' && fromCookie) {
      // Held in memory, so a restarted worker asks once more than it needs
      // to; that is the whole cost of not writing it down.
      if (Date.now() - disabledAskedAt < DISABLED_ASK_MS) return;
      disabledAskedAt = Date.now();
    }
    const asked = await callHost({ type: 'status', ...(await peer()) });
    if (!asked.ok || !asked.status?.bound || asked.status.enabled !== true) return;
  }
  await push();
}

let pushTimer = null;

// A timer does not hold the service worker open, so a torn-down worker can eat
// this one. Three seconds nearly always lands inside the grace period Chrome
// gives a worker after an event, and when it does not, the next cookie change or
// the daily alarm carries the same jar -- nothing is lost, only delayed.
function schedulePush() {
  if (pushTimer !== null) clearTimeout(pushTimer);
  pushTimer = setTimeout(() => {
    pushTimer = null;
    void backgroundPush({ fromCookie: true });
  }, PUSH_DEBOUNCE_MS);
}

function isYouTubeDomain(domain) {
  const host = domain.replace(/^\./, '').toLowerCase();
  return host === 'youtube.com' || host.endsWith('.youtube.com');
}

function ensureAlarm() {
  // A session that has not moved in a day is one the app would otherwise let go
  // stale; re-sending the same jar is what keeps it fresh without the user
  // having to think about it.
  chrome.alarms.create(REFRESH_ALARM, { periodInMinutes: 60 * 24 });
}

// One record per tab: `{ url, title, sawMedia, items }`. Events for one tab
// arrive interleaved -- a manifest and its first segments land within the same
// millisecond -- so every change to a tab's record waits for the one before
// it. Without the chain two reads see the same list and the second write
// drops the first item.
const chains = new Map();

// Tabs already known to be playing something, so the segment requests that
// arrive every few seconds for as long as a video plays cost a set lookup
// rather than a storage round trip each.
const playing = new Set();

function blankTab() {
  return { url: '', title: '', sawMedia: false, items: [] };
}

// A record once its tab has moved to another page, or a single-page site to a
// new address: what the last one played is not on this one, so only what was
// seen at the new address stays.
function movedTo(record, url, title) {
  return {
    url,
    title,
    sawMedia: false,
    items: record.items.filter((item) => samePage(item.seenOn, url)),
  };
}

async function readTab(tabId) {
  const key = `${TAB_PREFIX}${tabId}`;
  try {
    const stored = await chrome.storage.session.get(key);
    return stored[key] ?? null;
  } catch {
    return null;
  }
}

/**
 * Change a tab's record. `change(record, exists)` returns the new record, or
 * undefined to leave it as it is; the promise resolves to the record after.
 */
function updateTab(tabId, change) {
  const run = (chains.get(tabId) ?? Promise.resolve()).then(async () => {
    const stored = await readTab(tabId);
    const current = stored ?? blankTab();
    const next = await change(structuredClone(current), stored !== null);
    if (!next) return current;
    await chrome.storage.session.set({ [`${TAB_PREFIX}${tabId}`]: next });
    showCount(tabId, next);
    return next;
  });
  const settled = run.catch(() => null);
  chains.set(tabId, settled);
  void settled.then(() => {
    if (chains.get(tabId) === settled) chains.delete(tabId);
  });
  return settled;
}

function dropTab(tabId) {
  playing.delete(tabId);
  const run = (chains.get(tabId) ?? Promise.resolve()).then(() =>
    chrome.storage.session.remove(`${TAB_PREFIX}${tabId}`),
  );
  const settled = run.catch(() => null);
  chains.set(tabId, settled);
  void settled.then(() => {
    if (chains.get(tabId) === settled) chains.delete(tabId);
  });
}

// A record created by a request rather than by a navigation does not know its
// page yet; the tab does.
async function withPage(tabId, record) {
  if (record.url) return record;
  try {
    const tab = await chrome.tabs.get(tabId);
    record.url = tab.url ?? '';
    record.title = tab.title ?? '';
  } catch {
    // The tab closed between the request and now; the record goes with it.
  }
  return record;
}

// The toolbar badge counts what the popup would list, so a number appears
// when there is something to press and nothing appears when there is not.
function showCount(tabId, record, count = rows(record, []).length) {
  chrome.action.setBadgeText({ tabId, text: count > 0 ? String(count) : '' }).catch(() => {});
}

function notePlaying(tabId) {
  if (playing.has(tabId)) return;
  playing.add(tabId);
  void updateTab(tabId, async (record) => {
    if (record.sawMedia) return undefined;
    record.sawMedia = true;
    return withPage(tabId, record);
  });
}

function onHeaders(details) {
  if (details.tabId < 0) return;
  if (details.statusCode < 200 || details.statusCode > 299) return;

  const headers = headerMap(details.responseHeaders);
  const kind = classify({ url: details.url, contentType: headers['content-type'], type: details.type });
  if (!kind) return;
  if (kind === 'segment' || kind === 'noise') {
    notePlaying(details.tabId);
    return;
  }

  const size = sizeFrom(headers, details.statusCode);
  if (tooSmall(kind, size)) return;

  const url = dedupeKey(details.url);
  void updateTab(details.tabId, async (record) => {
    const known = record.items.find((item) => item.url === url);
    if (known) {
      if (typeof size !== 'number' || known.size === size) return undefined;
      known.size = size;
      return record;
    }
    await withPage(details.tabId, record);
    // A tab on a page this extension may not see, or one that has closed:
    // there is no list for it to go into.
    if (!record.url) return undefined;
    record.items.push({
      id: crypto.randomUUID().slice(0, 8),
      url,
      kind,
      frameId: details.frameId >= 0 ? details.frameId : 0,
      initiator: typeof details.initiator === 'string' ? details.initiator : '',
      isXhr: details.type === 'xmlhttprequest',
      contentType: headers['content-type'] ?? '',
      size: typeof size === 'number' ? size : null,
      seenOn: record.url,
      at: Date.now(),
    });
    record.items = record.items.slice(-MAX_ITEMS);
    return record;
  });
}

// Runs inside each frame of the page, in the extension's isolated world. It
// has to be self-contained: Chrome serialises the function's source, so
// nothing from this module is in scope when it runs.
function collect() {
  const web = (value) => (typeof value === 'string' && /^https?:/i.test(value) ? value : '');
  const meta = (name) =>
    document.querySelector(`meta[property="${name}"], meta[name="${name}"]`)?.getAttribute('content') ?? '';
  const absolute = (value) => {
    try {
      return value ? new URL(value, location.href).href : '';
    } catch {
      return '';
    }
  };
  const describe = (element, withPoster) => ({
    src: web(element.currentSrc || element.src),
    poster: withPoster ? web(element.poster) : '',
    duration: Number.isFinite(element.duration) ? element.duration : 0,
    width: element.videoWidth || 0,
    height: element.videoHeight || 0,
    drm: element.mediaKeys != null,
  });
  const isTop = window === window.top;
  return {
    href: location.href,
    isTop,
    title: isTop ? document.title : '',
    ogTitle: isTop ? meta('og:title') : '',
    ogImage: isTop ? web(absolute(meta('og:image'))) : '',
    ogUrl: isTop ? web(absolute(meta('og:url'))) : '',
    videos: [...document.querySelectorAll('video')].slice(0, 50).map((video) => describe(video, true)),
    audios: [...document.querySelectorAll('audio')].slice(0, 50).map((audio) => describe(audio, false)),
  };
}

// Also runs inside the page, in the frame that first asked for the manifest,
// so the request carries the Referer, Origin and cookies the player's did.
// `force-cache` lets the browser answer from what the player already fetched
// where it can, so most of these never reach the network at all.
async function fetchText(url, maxBytes, timeoutMs) {
  const controller = new AbortController();
  const timer = setTimeout(() => controller.abort(), timeoutMs);
  try {
    const response = await fetch(url, { signal: controller.signal, cache: 'force-cache' });
    if (!response.ok || !response.body) return null;
    const reader = response.body.getReader();
    const parts = [];
    let total = 0;
    let truncated = false;
    for (;;) {
      const { done, value } = await reader.read();
      if (done) break;
      const room = maxBytes - total;
      if (value.byteLength >= room) {
        parts.push(value.subarray(0, room));
        total += room;
        truncated = true;
        controller.abort();
        break;
      }
      parts.push(value);
      total += value.byteLength;
    }
    const bytes = new Uint8Array(total);
    let at = 0;
    for (const part of parts) {
      bytes.set(part, at);
      at += part.byteLength;
    }
    return { text: new TextDecoder().decode(bytes), truncated };
  } catch {
    return null;
  } finally {
    clearTimeout(timer);
  }
}

function withTimeout(promise, ms) {
  let timer;
  const expiry = new Promise((resolve) => {
    timer = setTimeout(() => resolve(undefined), ms);
  });
  return Promise.race([promise, expiry]).finally(() => clearTimeout(timer));
}

async function collectFrames(tabId) {
  try {
    const results = await withTimeout(
      chrome.scripting.executeScript({ target: { tabId, allFrames: true }, func: collect }),
      COLLECT_TIMEOUT_MS,
    );
    return (results ?? [])
      .filter((entry) => entry && entry.result && typeof entry.result === 'object')
      .map((entry) => normaliseFrame(entry.result, entry.frameId));
  } catch {
    // chrome:// pages, the Web Store and frames the browser will not let an
    // extension into. What the requests showed is still worth listing.
    return [];
  }
}

async function readManifest(tabId, item) {
  try {
    const results = await withTimeout(
      chrome.scripting.executeScript({
        target: { tabId, frameIds: [item.frameId] },
        func: fetchText,
        args: [item.url, MANIFEST_MAX_BYTES, MANIFEST_TIMEOUT_MS],
      }),
      MANIFEST_TIMEOUT_MS + 500,
    );
    const fetched = results?.[0]?.result;
    if (!fetched || typeof fetched.text !== 'string') return {};
    const info = item.kind === 'hls' ? parseHls(fetched.text, item.url) : parseDash(fetched.text, item.url);
    if (!info) return {};
    // A media playlist cut short at the size limit has a length that is only
    // the part that was read.
    if (fetched.truncated && info.master === false) info.durationSec = null;
    return info;
  } catch {
    return {};
  }
}

// Manifests are read once per item. A failure is remembered as an empty
// answer rather than retried on every open, because the usual reason is a
// server that will not answer this frame a second time, and retrying would
// keep every popup waiting on the same timeout.
async function readManifests(tabId, items) {
  const wanted = items
    .filter((item) => (item.kind === 'hls' || item.kind === 'dash') && !item.info)
    .slice(-MANIFESTS_PER_SCAN);
  const answers = await Promise.all(
    wanted.map(async (item) => [item.id, await readManifest(tabId, item)]),
  );
  return new Map(answers);
}

async function scan(message) {
  const tabId = message.tabId;
  if (!Number.isInteger(tabId) || tabId < 0) return { ok: false };

  let tab;
  try {
    tab = await chrome.tabs.get(tabId);
  } catch {
    return { ok: false };
  }
  const url = tab.url ?? '';
  const title = tab.title ?? '';

  // A service the app refuses by name is not worth reading the page of, and a
  // page this extension may not see -- the New Tab page, chrome:// pages, the
  // Web Store, for which Chrome gives no address -- has nothing to list.
  const service = protectedService(url);
  if (service || !url) {
    showCount(tabId, null, 0);
    return { ok: true, url, protectedService: service, rows: [] };
  }

  // The record counts for this page only if it is this page's: the popup can
  // open before the worker has heard that the tab moved on.
  const onPage = (record) => {
    if (!record) return { ...blankTab(), url, title };
    return samePage(record.url, url) ? record : movedTo(record, url, title);
  };

  const [frames, stored] = await Promise.all([collectFrames(tabId), readTab(tabId)]);
  let record = onPage(stored);

  const infos = await readManifests(tabId, record.items);
  if (infos.size > 0) {
    const attach = (current) => {
      for (const item of current.items) {
        if (infos.has(item.id)) item.info = infos.get(item.id);
      }
      return current;
    };
    // Written back so the badge can hide variants too and the next open does
    // not read the same manifests again; used from memory if that fails.
    const written = await updateTab(tabId, (current, exists) => (exists ? attach(current) : undefined));
    record = written ? onPage(written) : attach(structuredClone(record));
  }

  const page = { ...record, url, title: title || record.title };
  const list = rows(page, frames, { userAgent: navigator.userAgent });
  showCount(tabId, page, list.length);
  return { ok: true, url, protectedService: null, rows: list };
}

// The popup draws the switch from these three facts, so every answer to it
// carries all three. `linkOff` is the one the old popup never got, which is
// how a session the user had turned off came to be described as "another
// profile is connected".
// `signedIn` is only looked up while the switch is on, the one state whose
// line depends on it: with the switch off -- here or in the app -- the
// extension does not read cookies at all, not even to count them.
async function linkReply(result) {
  const linkOff = await isOff();
  let signedIn = false;
  if (result.ok && result.status?.bound && result.status.enabled === true && !linkOff) {
    try {
      signedIn = (await readJar()).signedIn;
    } catch {
      // A profile whose cookies cannot be read is, for this purpose, signed out.
    }
  }
  return { result, signedIn, linkOff };
}

async function status() {
  let result = await callHost({ type: 'status', ...(await peer()) });
  if (result.ok && result.status?.bound) {
    if (await isOff()) {
      // Turned off while the app could not be reached: finish the job now
      // that it can, rather than leave a session the user asked to drop.
      const forgotten = await callHost({ type: 'forget', ...(await peer()) });
      if (forgotten.ok) result = forgotten;
    } else if (result.status.enabled) {
      // Status carries nothing and changes nothing. Following it with a push
      // when this profile is the bound one is what heals a link that has gone
      // quiet, every time the popup opens.
      const pushed = await push();
      if (pushed.ok) result = pushed;
    }
  }
  return linkReply(result);
}

async function connect() {
  // Clearing the off flag first, so nothing of ours refuses the push below.
  // Claim before the push: a push from an unbound profile is refused, so
  // sending the jar before the binding exists would be handing over cookies
  // the app is about to throw away.
  await setOff(false);
  const claimed = await callHost({ type: 'claim', ...(await peer()) });
  if (!claimed.ok) return linkReply(claimed);
  const pushed = await push();
  return linkReply(pushed.ok ? pushed : claimed);
}

async function forget() {
  const result = await callHost({ type: 'forget', ...(await peer()) });
  // Set even when the app could not be reached: the user asked for this to
  // stop, and an app that is closed or gone is no reason to keep sending.
  await setOff(true);
  return linkReply(result);
}

// Hand one row to the app. The popup sends back the row it was given; only
// Contract A's fields go on, checked again here, because the wire format is
// this file's responsibility and not the popup's.
async function download(message) {
  const fields = toPayload(message.payload);
  if (!fields) return { result: { ok: false, code: 'malformed', message: '' } };
  return { result: await callHost({ type: 'download', ...(await peer()), ...fields }) };
}

// What the app would make of a row: the title it would save under, its own
// still, and the quality, container and rough size of the file -- read by the
// app's helper with the same analysis and the same plan a download runs, so
// the row promises what the download then delivers.
//
// Asked for when the popup shows a row, which is why the answer is kept: the
// helper runs the download engine to find out, which takes seconds, and the
// same row is usually shown again the next time the popup opens. Only answers
// that asking again would not change are kept -- a preview, or "protected" --
// and none for longer than PROBE_TTL_MS, after which a signed address may have
// expired and a live stream ended.
const PROBE_KEY = 'probes';
const PROBE_TTL_MS = 15 * 60 * 1000;
const PROBE_KEEP = 40;
const probing = new Map();
let probeWrites = Promise.resolve();

function shortText(value, limit) {
  return typeof value === 'string' && value.trim() !== '' && value.length <= limit ? value.trim() : null;
}

// The helper is ours, but its answer is about a page, so it is held to the
// same standard as anything else from one before the popup draws it.
function previewOf(raw) {
  if (!raw || typeof raw !== 'object') return null;
  const number = (value) => (typeof value === 'number' && Number.isFinite(value) && value > 0 ? value : null);
  const container = shortText(raw.container, 10);
  return {
    title: cleanTitle(raw.title) || null,
    thumbnail: isHttpUrl(raw.thumbnail) ? raw.thumbnail : null,
    durationSec: number(raw.durationSec),
    qualityLabel: shortText(raw.qualityLabel, 40),
    container: container && /^[a-z0-9]+$/i.test(container) ? container.toLowerCase() : null,
    estimatedBytes: number(raw.estimatedBytes),
    platform: shortText(raw.platform, 40),
    isLive: raw.isLive === true,
    audioOnly: raw.audioOnly === true,
  };
}

async function readProbes() {
  try {
    const stored = await chrome.storage.session.get(PROBE_KEY);
    const probes = stored[PROBE_KEY];
    return probes && typeof probes === 'object' ? probes : {};
  } catch {
    return {};
  }
}

function keepProbe(key, answer) {
  probeWrites = probeWrites
    .then(async () => {
      const probes = await readProbes();
      probes[key] = { at: Date.now(), answer };
      const newest = Object.entries(probes)
        .filter(([, entry]) => Date.now() - entry.at < PROBE_TTL_MS)
        .sort(([, a], [, b]) => b.at - a.at)
        .slice(0, PROBE_KEEP);
      await chrome.storage.session.set({ [PROBE_KEY]: Object.fromEntries(newest) });
    })
    .catch(() => {});
  return probeWrites;
}

async function probe(message) {
  const fields = toPayload(message.payload);
  if (!fields) return { preview: null, protected: false };
  const key = `${fields.kind}:${fields.url}`;

  const kept = (await readProbes())[key];
  if (kept && Date.now() - kept.at < PROBE_TTL_MS) return kept.answer;

  // Two opens of the popup in quick succession ask once.
  if (!probing.has(key)) {
    const asking = (async () => {
      const result = await callHost({ type: 'probe', ...(await peer()), ...fields });
      const answer = {
        preview: result.ok ? previewOf(result.preview) : null,
        protected: !result.ok && result.code === 'protected',
      };
      if (answer.preview || answer.protected) await keepProbe(key, answer);
      return answer;
    })().finally(() => probing.delete(key));
    probing.set(key, asking);
  }
  return probing.get(key);
}

const ACTIONS = { scan, status, download, probe, connect, forget };

chrome.webRequest.onHeadersReceived.addListener(
  onHeaders,
  { urls: ['<all_urls>'], types: ['media', 'xmlhttprequest', 'other', 'object'] },
  ['responseHeaders'],
);

chrome.tabs.onUpdated.addListener((tabId, change, tab) => {
  // A page this extension may not see -- the New Tab page, chrome:// pages,
  // the Web Store -- arrives with neither its address nor its title, which
  // Chrome leaves out for an extension without the `tabs` permission. So no
  // address changes; the tab has still moved on, and what was recorded
  // belongs to the page before.
  if (tab.url === undefined) {
    if (change.status === 'loading') dropTab(tabId);
    return;
  }
  const loaded = change.status === 'complete';
  if (change.url === undefined && change.title === undefined && !loaded) return;
  if (change.url !== undefined) playing.delete(tabId);
  void updateTab(tabId, (record, exists) => {
    // A tab that has never played anything has nothing to keep up to date,
    // but a count the popup put on it while it was open belongs to the page
    // it was counted on.
    if (!exists) {
      if (change.url !== undefined) showCount(tabId, null, 0);
      return undefined;
    }
    if (change.url !== undefined && !samePage(change.url, record.url)) {
      return movedTo(record, change.url, tab.title ?? '');
    }
    const title = tab.title ?? record.title;
    const url = change.url ?? record.url;
    if (title === record.title && url === record.url) {
      // Chrome clears a tab's badge whenever a new document loads in it, and
      // a reload of the same address changes nothing here that would draw it
      // again, so it is put back once the page has loaded.
      if (loaded) showCount(tabId, record);
      return undefined;
    }
    return { ...record, url, title };
  });
});

chrome.tabs.onRemoved.addListener((tabId) => dropTab(tabId));

// Prerendering and instant pages swap one tab for another under the same
// window position; what was recorded for the hidden one is not what is shown.
chrome.tabs.onReplaced.addListener((_added, removed) => dropTab(removed));

chrome.runtime.onInstalled.addListener((details) => {
  ensureAlarm();
  // Opening the page ourselves is the whole reason this design needs no pairing
  // code: the first thing the user sees is the instructions, rather than a
  // toolbar icon Chrome has already hidden behind the puzzle-piece button. Only
  // on a fresh install -- an update is not a moment anyone asked to be taught.
  if (details.reason === 'install') {
    // A new install is a new profile id that no app has bound, so the cookie
    // listener has nothing to ask about until the switch is turned on.
    void chrome.storage.local.set({ [LINK_KEY]: 'unbound' });
    chrome.tabs.create({ url: chrome.runtime.getURL('welcome.html') });
  } else if (details.reason === 'update') {
    // 1.0.2 kept the host's last answer here. Nothing reads it any more, and
    // what this extension stores is meant to be exactly what the privacy
    // policy lists.
    void chrome.storage.local.remove('lastResult').catch(() => {});
  }
});

chrome.runtime.onStartup.addListener(() => {
  ensureAlarm();
  void backgroundPush();
});

chrome.alarms.onAlarm.addListener((alarm) => {
  if (alarm.name === REFRESH_ALARM) void backgroundPush();
});

// With access to every site this fires for every cookie the browser writes, so
// the test is the first thing it does: only YouTube's own cookies are a reason
// to send the session again.
chrome.cookies.onChanged.addListener((change) => {
  if (!isYouTubeDomain(change.cookie.domain)) return;
  schedulePush();
});

// Only this extension's own pages may ask for anything, and only by name. A
// page in a tab never can: nothing here injects a script that would listen,
// and a message from anywhere else is not answered at all.
chrome.runtime.onMessage.addListener((message, sender, respond) => {
  if (sender.id !== chrome.runtime.id || sender.tab !== undefined) return false;
  const action = message?.action;
  if (typeof action !== 'string' || !Object.hasOwn(ACTIONS, action)) return false;
  ACTIONS[action](message).then(respond, () => respond(null));
  return true;
});

// The badge in the app's accent, dark ink on the bright orange as in the
// app's dark theme, where it reads at 8:1.
chrome.action.setBadgeBackgroundColor({ color: '#ff8a4c' }).catch(() => {});
chrome.action.setBadgeTextColor({ color: '#1d0e05' }).catch(() => {});

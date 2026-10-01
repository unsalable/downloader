// The popup: what this tab is playing, a button to hand each one to the app,
// and the YouTube session switch.
//
// It holds no state worth keeping and talks to no one but the service worker,
// which owns the native messaging channel -- so the popup closing halfway
// through an exchange, which it does the moment the app takes focus, cannot
// leave one in progress.
//
// Everything a page supplied (titles, image addresses) is untrusted. It goes
// into the DOM as text through textContent, and images load only from http
// and https addresses; nothing here assigns HTML.

import { displayHost, formatDuration, formatEstimate, isHttpUrl } from './media.js';

const t = (key) => chrome.i18n.getMessage(key);
const el = (id) => document.getElementById(id);

const SVG = 'http://www.w3.org/2000/svg';

// The kind of each row, in the words the meta line uses for it.
const LABELS = {
  stream: 'kindStream',
  video: 'kindVideo',
  audio: 'kindAudio',
  page: 'kindPage',
  protected: 'kindProtected',
};

// Long enough to register as "it is doing something", short enough that a
// helper which answers at once does not look slow.
const MIN_SENDING_MS = 300;

// Each detail costs the app's helper a run of the download engine, so only the
// rows at the top are asked about, and two at a time.
const PROBE_ROWS = 4;
const PROBE_PARALLEL = 2;

// The labels a plan falls back on when the source named no quality. They say
// nothing the kind of the row does not already say.
const VAGUE_QUALITY = new Set(['video', 'audio', 'image']);

// What the app's answer means for this popup, as one of three sentences.
// `forbidden`, `version` and `malformed` all come down to an app older than
// this extension: one that does not know the store copy's id, speaks an older
// wire, or does not know what a download request is.
function problemOf(result) {
  if (!result) return 'unreachable';
  if (result.ok) return result.status?.canDownload === true ? null : 'update';
  switch (result.code) {
    case 'notInstalled':
      return 'notInstalled';
    case 'forbidden':
    case 'version':
    case 'malformed':
      return 'update';
    default:
      return 'unreachable';
  }
}

const NOTICES = {
  notInstalled: { text: 'noticeNotInstalled', retry: true },
  update: { text: 'noticeUpdate', retry: false },
  unreachable: { text: 'noticeUnreachable', retry: true },
};

let problem = null;

function showProblem(next) {
  problem = next;
  const notice = NOTICES[next];
  el('notice').hidden = !notice;
  if (notice) {
    el('noticeText').textContent = t(notice.text);
    el('noticeAction').hidden = !notice.retry;
  }
  // Every button would fail the same way the check just did.
  for (const button of document.querySelectorAll('.get:not([data-state])')) {
    button.disabled = problem !== null;
  }
}

function glyph(paths, viewBox = '0 0 24 24') {
  const svg = document.createElementNS(SVG, 'svg');
  svg.setAttribute('viewBox', viewBox);
  svg.setAttribute('aria-hidden', 'true');
  for (const attrs of paths) {
    const path = document.createElementNS(SVG, 'path');
    for (const [name, value] of Object.entries(attrs)) path.setAttribute(name, value);
    svg.append(path);
  }
  return svg;
}

const playGlyph = () => glyph([{ d: 'M8 5.5v13l10.5-6.5z', fill: 'currentColor' }]);
const checkGlyph = () =>
  glyph([
    {
      d: 'm5 12.5 4.5 4.5L19 7.5',
      fill: 'none',
      stroke: 'currentColor',
      'stroke-width': '2.25',
      'stroke-linecap': 'round',
      'stroke-linejoin': 'round',
    },
  ]);

// A picture over the play tile. A new one is laid over the old only once it
// has loaded, so a sharper still replacing a blurry one never flashes the tile
// in between, and one that fails to load leaves whatever was there.
function showPicture(box, src) {
  if (!isHttpUrl(src) || box.dataset.src === src) return;
  box.dataset.src = src;
  const img = document.createElement('img');
  img.alt = '';
  img.decoding = 'async';
  img.referrerPolicy = 'no-referrer';
  img.addEventListener('load', () => {
    if (box.dataset.src !== src) {
      img.remove();
      return;
    }
    img.classList.add('loaded');
    for (const old of box.querySelectorAll('img')) if (old !== img) old.remove();
  });
  img.addEventListener('error', () => img.remove());
  img.src = src;
  box.append(img);
}

function pictureBox(className, src) {
  const box = document.createElement('div');
  box.className = className;
  box.append(playGlyph());
  showPicture(box, src);
  return box;
}

function line(className, text, id) {
  const p = document.createElement('p');
  p.className = className;
  p.textContent = text;
  if (id) p.id = id;
  return p;
}

// What the row knows before the app has said anything: its kind, and the
// quality, size or length the page gave away.
function firstMeta(meta) {
  const label = meta.label === 'site' ? meta.site : t(LABELS[meta.label] ?? 'kindVideo');
  return [label, ...(meta.parts ?? [])].filter(Boolean).join(' · ');
}

// What the app said it would download, in the order its own Home screen
// says it -- "1080p - MKV" there, "1080p · MKV · ~82 MB" here. The length
// is added on the large card, where there is room for it; in a row of the
// list it would push the size out of sight.
function detailMeta(preview, row, { withLength = false } = {}) {
  const quality = preview.qualityLabel && !VAGUE_QUALITY.has(preview.qualityLabel.toLowerCase())
    ? preview.qualityLabel
    : null;
  const length = withLength && !preview.isLive
    ? formatDuration(preview.durationSec ?? row.durationSec ?? 0)
    : '';
  return [
    preview.isLive ? t('kindLive') : null,
    quality,
    preview.container ? preview.container.toUpperCase() : null,
    formatEstimate(preview.estimatedBytes) || null,
    length || null,
  ]
    .filter(Boolean)
    .join(' · ');
}

async function send(button, row) {
  button.disabled = true;
  button.dataset.state = 'sending';
  const started = Date.now();
  const reply = await ask('download', { payload: row.payload });
  const wait = MIN_SENDING_MS - (Date.now() - started);
  if (wait > 0) await new Promise((resolve) => setTimeout(resolve, wait));

  const result = reply?.result ?? null;
  if (result?.ok) {
    // Sent stays sent: pressing it again would queue the same download twice.
    // In a row of the list the check says it alone -- the word would take the
    // room the row's details need -- and the word is still its name.
    button.dataset.state = 'sent';
    button.setAttribute('aria-label', t('btnSent'));
    if (button.closest('.row')) {
      button.title = t('btnSent');
      button.replaceChildren(checkGlyph());
    } else {
      const label = document.createElement('span');
      label.textContent = t('btnSent');
      button.replaceChildren(checkGlyph(), label);
    }
    showProblem(null);
    return;
  }
  delete button.dataset.state;
  button.disabled = false;
  showProblem(problemOf(result) ?? 'unreachable');
}

function downloadButton(row, describedBy) {
  const button = document.createElement('button');
  button.type = 'button';
  button.className = 'get';
  button.textContent = t('btnDownload');
  button.setAttribute('aria-describedby', describedBy);
  button.disabled = problem !== null;
  button.addEventListener('click', () => send(button, row));
  return button;
}

// One row on the screen and the nodes the app's answer will change.
function drawn(row, node, picture, title, meta, button, feature = false) {
  return { row, node, picture, title, meta, button, feature };
}

function rowNode(row, index) {
  const node = document.createElement('div');
  node.className = 'row';

  const text = document.createElement('div');
  text.className = 'text';
  const titleId = `row-${index}`;
  const title = line('title', row.title, titleId);
  const meta = line('meta', row.protected ? t('kindProtected') : firstMeta(row.meta));
  text.append(title, meta);
  const picture = pictureBox('thumb', row.thumbnail);
  node.append(picture, text);

  // A protected row has nothing to press; its meta line says why.
  const button = row.protected ? null : downloadButton(row, titleId);
  if (button) node.append(button);
  return drawn(row, node, picture, title, meta, button);
}

// The page is one video: shown the way the app shows one, its still large
// above its title, and the button across the card's width.
function featureNode(row) {
  const node = document.createElement('div');
  node.className = 'feature';
  const picture = pictureBox('poster', row.thumbnail);
  const title = line('title', row.title, 'row-0');
  const meta = line('meta', row.protected ? t('kindProtected') : firstMeta(row.meta));
  const text = document.createElement('div');
  text.className = 'text';
  text.append(title, meta);
  node.append(picture, text);
  const button = row.protected ? null : downloadButton(row, 'row-0');
  if (button) node.append(button);
  return drawn(row, node, picture, title, meta, button, true);
}

function emptyNode(titleKey, bodyKey) {
  const box = document.createElement('div');
  box.className = 'empty';
  box.append(line('title', t(titleKey)), line('meta', t(bodyKey)));
  return box;
}

// The app's answer, laid into a row. Its title and still go into what İndir
// sends as well, so the download is named and pictured in the app the moment
// it arrives rather than when it finishes.
function apply(item, answer) {
  item.meta.classList.remove('pending');
  const { row } = item;
  if (answer?.protected) {
    item.button?.remove();
    item.meta.textContent = t('kindProtected');
    return;
  }
  const preview = answer?.preview;
  if (!preview) {
    item.meta.textContent = firstMeta(row.meta);
    return;
  }
  if (preview.title) {
    item.title.textContent = preview.title;
    row.payload.title = preview.title;
  }
  if (preview.thumbnail) {
    showPicture(item.picture, preview.thumbnail);
    row.payload.thumbnail = preview.thumbnail;
  }
  item.meta.textContent = detailMeta(preview, row, { withLength: item.feature }) || firstMeta(row.meta);
}

// Ask about the rows at the top, a couple at a time, and fill each in as its
// answer comes. Until then its meta line is a quiet placeholder, as the app
// draws one while it reads a link.
async function describe(items) {
  const wanted = items.filter((item) => !item.row.protected).slice(0, PROBE_ROWS);
  for (const item of wanted) item.meta.classList.add('pending');
  const queue = [...wanted];
  const worker = async () => {
    for (let item = queue.shift(); item; item = queue.shift()) {
      apply(item, await ask('probe', { payload: item.row.payload }));
    }
  };
  await Promise.all(Array.from({ length: PROBE_PARALLEL }, worker));
}

function renderList(reply) {
  const list = el('list');
  list.classList.remove('single');
  let items = [];
  if (reply?.protectedService) {
    list.replaceChildren(emptyNode('protectedTitle', 'protectedBody'));
  } else if (Array.isArray(reply?.rows) && reply.rows.length === 1) {
    list.classList.add('single');
    items = [featureNode(reply.rows[0])];
    list.replaceChildren(items[0].node);
  } else if (Array.isArray(reply?.rows) && reply.rows.length > 1) {
    items = reply.rows.map(rowNode);
    list.replaceChildren(...items.map((item) => item.node));
  } else {
    list.replaceChildren(emptyNode('emptyTitle', 'emptyBody'));
  }
  list.hidden = false;
  if (items.length > 0) void describe(items);
}

// The switch is on when this profile holds the binding and the user has not
// turned it off; the line under the title says what that means right now.
function renderLink(reply) {
  const result = reply?.result;
  const status = result?.ok ? result.status : null;
  let on = false;
  let usable = true;
  let key = 'linkOffHint';

  if (!status) {
    usable = false;
  } else if (!status.enabled) {
    usable = false;
    key = 'linkDisabled';
  } else if (reply.linkOff) {
    key = 'linkOffHint';
  } else if (!status.bound) {
    key = status.boundBrowser ? 'linkOther' : 'linkOffHint';
  } else {
    on = true;
    key = !reply.signedIn ? 'linkSignedOut' : status.session === 'fresh' ? 'linkOn' : 'linkStale';
  }

  const toggle = el('linkSwitch');
  toggle.setAttribute('aria-checked', String(on));
  toggle.disabled = !usable;
  el('linkLine').textContent = t(key);
}

// The worker's answer, or null when there was none to be had.
async function ask(action, fields = {}) {
  try {
    return await chrome.runtime.sendMessage({ action, ...fields });
  } catch {
    return null;
  }
}

async function checkApp() {
  const reply = await ask('status');
  showProblem(problemOf(reply?.result));
  renderLink(reply);
}

el('linkSwitch').addEventListener('click', async () => {
  const toggle = el('linkSwitch');
  const turningOn = toggle.getAttribute('aria-checked') !== 'true';
  // The knob moves at once; the answer then sets it where it really is.
  toggle.setAttribute('aria-checked', String(turningOn));
  toggle.disabled = true;
  const reply = await ask(turningOn ? 'connect' : 'forget');
  if (reply?.result?.ok) {
    showProblem(problemOf(reply.result));
    renderLink(reply);
  } else {
    // A refusal carries no status to draw the switch from; asking again does,
    // and says whether the app is gone or only its setting is off.
    await checkApp();
  }
});

el('noticeAction').textContent = t('btnRetry');
el('noticeAction').addEventListener('click', () => checkApp());

async function start() {
  let tab = null;
  try {
    [tab] = await chrome.tabs.query({ active: true, currentWindow: true });
  } catch {
    // No tab to speak of, as on a browser's own pages; the list says so.
  }
  const host = displayHost(tab?.url);
  el('host').textContent = host;
  el('host').hidden = host === '';

  void checkApp();
  const reply = Number.isInteger(tab?.id) ? await ask('scan', { tabId: tab.id }) : null;
  renderList(reply);
}

void start();

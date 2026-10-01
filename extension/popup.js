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

import { displayHost, isHttpUrl } from './media.js';

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

function thumb(src) {
  const box = document.createElement('div');
  box.className = 'thumb';
  box.append(playGlyph());
  if (isHttpUrl(src)) {
    const img = document.createElement('img');
    img.alt = '';
    img.decoding = 'async';
    img.referrerPolicy = 'no-referrer';
    img.addEventListener('load', () => img.classList.add('loaded'));
    // A broken image leaves the play tile it was covering, not a broken icon.
    img.addEventListener('error', () => img.remove());
    img.src = src;
    box.append(img);
  }
  return box;
}

function metaLine(meta) {
  const label = meta.label === 'site' ? meta.site : t(LABELS[meta.label] ?? 'kindVideo');
  return [label, ...(meta.parts ?? [])].filter(Boolean).join(' · ');
}

function line(className, text, id) {
  const p = document.createElement('p');
  p.className = className;
  p.textContent = text;
  if (id) p.id = id;
  return p;
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
    button.dataset.state = 'sent';
    const label = document.createElement('span');
    label.textContent = t('btnSent');
    button.replaceChildren(checkGlyph(), label);
    showProblem(null);
    return;
  }
  delete button.dataset.state;
  button.disabled = false;
  showProblem(problemOf(result) ?? 'unreachable');
}

function rowNode(row, index) {
  const node = document.createElement('div');
  node.className = 'row';

  const text = document.createElement('div');
  text.className = 'text';
  const titleId = `row-${index}`;
  text.append(line('title', row.title, titleId), line('meta', metaLine(row.meta)));
  node.append(thumb(row.thumbnail), text);

  // A protected row has nothing to press; its meta line says why.
  if (!row.protected) {
    const button = document.createElement('button');
    button.type = 'button';
    button.className = 'get';
    button.textContent = t('btnDownload');
    button.setAttribute('aria-describedby', titleId);
    button.disabled = problem !== null;
    button.addEventListener('click', () => send(button, row));
    node.append(button);
  }
  return node;
}

function emptyNode(titleKey, bodyKey) {
  const box = document.createElement('div');
  box.className = 'empty';
  box.append(line('title', t(titleKey)), line('meta', t(bodyKey)));
  return box;
}

function renderList(reply) {
  const list = el('list');
  if (reply?.protectedService) {
    list.replaceChildren(emptyNode('protectedTitle', 'protectedBody'));
  } else if (Array.isArray(reply?.rows) && reply.rows.length > 0) {
    list.replaceChildren(...reply.rows.map(rowNode));
  } else {
    list.replaceChildren(emptyNode('emptyTitle', 'emptyBody'));
  }
  list.hidden = false;
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

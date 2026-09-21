// The popup: one screen that has to tell eleven situations apart and give each
// of them the one sentence, and the one button, that gets the user out of it.
//
// It holds no state of its own and talks to no one but the service worker. The
// worker owns the native messaging channel, so the popup closing halfway
// through an exchange cannot leave one in progress.

const GOOGLE_ORIGIN = 'https://*.google.com/*';
const ACCOUNT_PERMISSIONS = ['identity', 'identity.email'];

const el = (id) => document.getElementById(id);

// The way out of each state, and at most one way round it.
//
// A button earns its place by being able to change the situation it is shown
// in. Retry and Open Universal Downloader both travel the same native
// messaging channel, so neither belongs in a state where that channel is what
// failed -- pressing Open there would run the exchange that just failed and
// land on this screen again. That is what leaves the three transport failures
// and `version` -- which the host refuses before it even reads the request
// type -- with Retry alone.
//
// `connected` has no way out because it is not a situation anyone needs to get
// out of. Turning the link off is offered there, quietly, and nothing else.
const STATES = {
  connected: { tone: 'ok', also: 'btnForget' },
  quiet: { tone: 'warn', act: 'btnRetry', also: 'btnForget' },
  signedOut: { tone: 'warn', act: 'btnRetry', also: 'btnForget' },
  unpaired: { tone: 'warn', act: 'btnConnect' },
  disabled: { tone: 'warn', act: 'btnOpenApp', also: 'btnRetry' },
  notInstalled: { tone: 'bad', act: 'btnRetry' },
  forbidden: { tone: 'bad', act: 'btnRetry' },
  unreachable: { tone: 'bad', act: 'btnRetry' },
  version: { tone: 'bad', act: 'btnRetry' },
  internal: { tone: 'bad', act: 'btnOpenApp', also: 'btnRetry' },
  malformed: { tone: 'bad', act: 'btnRetry' },
};

const ALL_BUTTONS = ['btnConnect', 'btnOpenApp', 'btnRetry', 'btnForget'];

// Order matters. A signed-out profile that is also unpaired is told to connect
// first, because connecting is the step it is standing on; being told to sign in
// and then finding the button still says Connect is how a two-fault screen
// wastes a user's afternoon.
function stateOf(reply) {
  const { result, signedIn } = reply;
  if (!result.ok) {
    return Object.hasOwn(STATES, result.code) ? result.code : 'unreachable';
  }
  const status = result.status;
  if (!status) return 'unreachable';
  if (!status.enabled) return 'disabled';
  if (!status.bound) return 'unpaired';
  if (!signedIn) return 'signedOut';
  return status.session === 'fresh' ? 'connected' : 'quiet';
}

function relativeTime(epochSeconds) {
  if (typeof epochSeconds !== 'number') return chrome.i18n.getMessage('timeNever');
  const seconds = Math.max(0, Math.floor(Date.now() / 1000) - epochSeconds);
  if (seconds < 90) return chrome.i18n.getMessage('timeJustNow');
  if (seconds < 3600) {
    return chrome.i18n.getMessage('timeMinutes', [String(Math.round(seconds / 60))]);
  }
  if (seconds < 86400) {
    return chrome.i18n.getMessage('timeHours', [String(Math.round(seconds / 3600))]);
  }
  return chrome.i18n.getMessage('timeDays', [String(Math.round(seconds / 86400))]);
}

function showFact(row, value, target) {
  const present = typeof value === 'string' && value !== '';
  el(row).hidden = !present;
  if (present) el(target).textContent = value;
  return present;
}

// Replay the entrance on an element that has already been through it. The read
// of `offsetWidth` is what makes this work: without it the two class changes
// land in one frame and the browser has nothing to restart.
function enter(node) {
  node.classList.remove('enter');
  void node.offsetWidth;
  node.classList.add('enter');
}

// The confirmation takes the action row's place rather than appearing beneath
// it, so the question is asked where the button that asked it was standing.
function showConfirm(asking) {
  el('actions').hidden = asking;
  el('confirm').hidden = !asking;
  enter(asking ? el('confirm') : el('actions'));
}

// What the last render put on screen, so a state that has not changed is not
// animated again. Pressing Retry on a screen that stays the same should look
// like nothing happened, because nothing did.
let shown = null;

function render(reply) {
  const state = stateOf(reply);
  const status = reply.result.status ?? null;

  const key = `s${state[0].toUpperCase()}${state.slice(1)}`;
  el('dot').dataset.tone = STATES[state].tone;
  el('title').textContent = chrome.i18n.getMessage(`${key}Title`);
  el('body').textContent = chrome.i18n.getMessage(`${key}Body`);

  for (const id of ALL_BUTTONS) {
    el(id).hidden = id !== STATES[state].act && id !== STATES[state].also;
    el(id).classList.toggle('primary', id === STATES[state].act);
  }

  el('actions').hidden = false;
  el('confirm').hidden = true;

  // Everything below is diagnosis rather than instruction, which is why it
  // lives in the fold. The host path is there because it is the only way a
  // user could ever notice that the registry entry now names something other
  // than the app they installed.
  showFact('factAccount', status?.accountHint, 'valueAccount');
  showFact(
    'factSent',
    typeof status?.lastPushAt === 'number' ? relativeTime(status.lastPushAt) : undefined,
    'valueSent',
  );
  showFact('factAppVersion', status?.appVersion, 'valueAppVersion');
  el('pathHint').hidden = !showFact('factPath', status?.hostPath, 'valuePath');

  // The pin hint teaches where Chrome put the icon, which is worth saying
  // while the user is still trying to get connected and is furniture
  // afterwards. Bound is the line: past it they have found this popup.
  el('pin').hidden = Boolean(status?.bound);

  // The two optional permissions are only an answer to "it is connected and
  // YouTube still will not let me". Offering them while the app is unreachable
  // would be asking for a wider grant to fix something they cannot fix.
  el('help').hidden = !['connected', 'quiet', 'signedOut'].includes(state);

  if (state !== shown) enter(el('panel'));
  shown = state;
}

function setBusy(busy) {
  for (const id of [...ALL_BUTTONS, 'btnForgetYes', 'btnForgetNo']) {
    el(id).disabled = busy;
  }
}

const NO_ANSWER = { result: { ok: false, code: 'unreachable' }, signedIn: false };

async function run(action) {
  setBusy(true);
  try {
    render((await chrome.runtime.sendMessage({ action })) ?? NO_ANSWER);
  } catch {
    // The service worker did not answer. From here that is indistinguishable
    // from the app not answering, and carries the same advice, so the screen
    // says that rather than staying on "Checking" forever.
    render(NO_ANSWER);
  } finally {
    setBusy(false);
  }
}

// Each optional permission is offered only after the connection is working and
// has still not produced what the user wanted. Asking at install for the right
// to read every google.com cookie is what gets an extension like this refused,
// and neither grant is needed by the common case.
async function reflectGrants() {
  const google = await chrome.permissions.contains({ origins: [GOOGLE_ORIGIN] });
  el('btnGrantGoogle').hidden = google;
  el('grantGoogle').querySelector('.granted').hidden = !google;

  let account = false;
  try {
    account = await chrome.permissions.contains({ permissions: ACCOUNT_PERMISSIONS });
  } catch {
    // A build that will not take these as optional; the offer is simply absent.
  }
  el('btnGrantAccount').hidden = account;
  el('grantAccount').querySelector('.granted').hidden = !account;
}

// `request` has to be the first thing this handler does: anything awaited before
// it spends the user gesture Chrome requires, and the prompt never appears.
async function ask(descriptor) {
  try {
    if (await chrome.permissions.request(descriptor)) {
      await reflectGrants();
      await run('status');
    }
  } catch {
    await reflectGrants();
  }
}

function fold(section, head) {
  el(head).addEventListener('click', () => {
    const open = !el(section).hasAttribute('data-open');
    el(section).toggleAttribute('data-open', open);
    el(head).setAttribute('aria-expanded', String(open));
  });
}

fold('help', 'helpHead');
fold('details', 'detailsHead');

el('btnRetry').addEventListener('click', () => run('status'));
el('btnConnect').addEventListener('click', () => run('connect'));
el('btnOpenApp').addEventListener('click', () => run('openApp'));

// Turning the connection off deletes a session the user may have spent a
// sign-in getting, so it asks -- inline rather than through `confirm()`, which
// a popup is not guaranteed to survive.
el('btnForget').addEventListener('click', () => showConfirm(true));
el('btnForgetNo').addEventListener('click', () => showConfirm(false));
el('btnForgetYes').addEventListener('click', () => run('forget'));

el('btnGrantGoogle').addEventListener('click', () => ask({ origins: [GOOGLE_ORIGIN] }));
el('btnGrantAccount').addEventListener('click', () => ask({ permissions: ACCOUNT_PERMISSIONS }));

// The extension's own version comes from the manifest rather than the host, so
// it is the one line of the fold that is there even when nothing answered.
el('valueExtVersion').textContent = chrome.runtime.getManifest().version;

void reflectGrants();
void run('status');

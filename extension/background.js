// The browser half of the link: read the YouTube session, hand it to the app.
//
// Everything here goes through one-shot `sendNativeMessage` rather than a
// long-lived `connectNative` port. A port would be the obvious choice for a
// connection that lives as long as the browser, but an MV3 service worker does
// not live that long -- Chrome tears it down after about thirty seconds idle and
// takes the port with it. Betting on the worker's lifetime is how this kind of
// extension ends up "working until you leave it alone for a minute". A one-shot
// send starts the host, says one thing, reads one answer and is done, and the
// worker may be collected the instant it returns.
//
// The wire format is src-tauri/src/bridge/protocol.rs. Field names come from
// there, not from taste.

const HOST_NAME = 'com.universaldownloader.bridge';
const WIRE_VERSION = 1;

// Long enough that signing in -- which writes a dozen cookies in a burst --
// produces one push rather than a dozen, short enough that the app is usable by
// the time the user has switched windows.
const PUSH_DEBOUNCE_MS = 3000;

const REFRESH_ALARM = 'refresh';
const GOOGLE_ORIGIN = 'https://*.google.com/*';
const OFF_KEY = 'linkOff';

// A jar can be full of cookies and still belong to a signed-out browser:
// YouTube sets visitor and preference cookies for everyone. These are the ones
// that actually carry a sign-in, so their presence is what `signedIn` means.
// LOGIN_INFO is in the list because it is YouTube's own, and a profile can hold
// it when the google.com cookies are not readable with the permissions we ask
// for at install.
const SESSION_COOKIE_NAMES = new Set([
  'SID',
  '__Secure-1PSID',
  '__Secure-3PSID',
  'LOGIN_INFO',
]);

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

// Whether the user turned the link off from the popup.
//
// The app forgetting the session is not enough on its own: the host refuses a
// push from an unbound profile, but this extension would keep offering one on
// every cookie change, and an off switch the user has to keep pressing is not
// an off switch. Kept here rather than inferred from the host's answers so it
// also holds while the app is closed or uninstalled.
async function isOff() {
  const stored = await chrome.storage.local.get(OFF_KEY);
  return stored[OFF_KEY] === true;
}

function setOff(off) {
  return chrome.storage.local.set({ [OFF_KEY]: off });
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

async function googleGranted() {
  try {
    return await chrome.permissions.contains({ origins: [GOOGLE_ORIGIN] });
  } catch {
    return false;
  }
}

// Everything the profile holds for the domains we are allowed to read.
//
// google.com is only included once the user has granted the optional host
// permission from the popup. The youtube.com jar is enough on its own for most
// accounts, and asking for Google-wide cookies at install is what gets an
// extension like this refused.
async function readJar() {
  const domains = ['youtube.com'];
  if (await googleGranted()) domains.push('google.com');

  const jars = await Promise.all(domains.map((domain) => chrome.cookies.getAll({ domain })));
  const cookies = jars.flat().map(wireCookie);
  const signedIn = cookies.some(
    (cookie) => SESSION_COOKIE_NAMES.has(cookie.name) && cookie.value !== '',
  );

  return { cookies, signedIn };
}

// A hint at which account this is, for the app to display -- never the address
// itself. Masking here rather than in the app is the point: the full address
// never leaves the browser, so no later mistake in the app can expose it.
async function accountHint() {
  if (!chrome.identity) return undefined;
  let granted = false;
  try {
    granted = await chrome.permissions.contains({
      permissions: ['identity', 'identity.email'],
    });
  } catch {
    return undefined;
  }
  if (!granted) return undefined;

  const info = await new Promise((resolve) => {
    try {
      chrome.identity.getProfileUserInfo({ accountStatus: 'ANY' }, (result) => {
        void chrome.runtime.lastError;
        resolve(result);
      });
    } catch {
      resolve(undefined);
    }
  });

  const email = info?.email;
  if (typeof email !== 'string') return undefined;
  const at = email.lastIndexOf('@');
  if (at < 1) return undefined;
  return `${email[0]}•••${email.slice(at)}`;
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
          resolve({ ok: true, status: response.status ?? null });
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

// Every exchange with the host goes through here so the last outcome is always
// on disk. The popup renders that when it cannot reach the host at all, which
// is exactly the case where it has nothing else to say.
async function callHost(request) {
  const result = await sendNative(request);
  const record = {
    ok: result.ok,
    code: result.ok ? null : result.code,
    at: Math.floor(Date.now() / 1000),
  };
  try {
    await chrome.storage.local.set({ lastResult: record });
  } catch {
    // Storage being unavailable is not a reason to drop the answer we have.
  }
  return result;
}

async function requestStatus() {
  return callHost({ type: 'status', ...(await peer()) });
}

async function push() {
  if (await isOff()) return { ok: false, error: { code: 'disabled' } };

  const jar = await readJar();
  const request = {
    type: 'push',
    ...(await peer()),
    signedIn: jar.signedIn,
    capturedAt: Math.floor(Date.now() / 1000),
    cookies: jar.cookies,
  };
  const hint = await accountHint();
  if (hint) request.accountHint = hint;
  return callHost(request);
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
    void push();
  }, PUSH_DEBOUNCE_MS);
}

function isLinkedDomain(domain) {
  const host = domain.replace(/^\./, '').toLowerCase();
  return (
    host === 'youtube.com' ||
    host.endsWith('.youtube.com') ||
    host === 'google.com' ||
    host.endsWith('.google.com')
  );
}

function ensureAlarm() {
  // A session that has not moved in a day is one the app would otherwise let go
  // stale; re-sending the same jar is what keeps it fresh without the user
  // having to think about it.
  chrome.alarms.create(REFRESH_ALARM, { periodInMinutes: 60 * 24 });
}

chrome.runtime.onInstalled.addListener((details) => {
  ensureAlarm();
  // Opening the page ourselves is the whole reason this design needs no pairing
  // code: the first thing the user sees is the instructions, rather than a
  // toolbar icon Chrome has already hidden behind the puzzle-piece button. Only
  // on a fresh install -- an update is not a moment anyone asked to be taught.
  if (details.reason === 'install') {
    chrome.tabs.create({ url: chrome.runtime.getURL('welcome.html') });
  }
});

chrome.runtime.onStartup.addListener(() => {
  ensureAlarm();
  void push();
});

chrome.alarms.onAlarm.addListener((alarm) => {
  if (alarm.name === REFRESH_ALARM) void push();
});

chrome.cookies.onChanged.addListener((change) => {
  if (!isLinkedDomain(change.cookie.domain)) return;
  schedulePush();
});

async function act(action) {
  switch (action) {
    case 'connect': {
      // Claim first: a push from an unbound profile is refused, so sending the
      // jar before the binding exists would be handing over cookies the app is
      // about to throw away.
      // Clearing the off flag first, so the push below is not refused by our
      // own guard. Connect is the user asking for this.
      await setOff(false);
      const claimed = await callHost({ type: 'claim', ...(await peer()) });
      return claimed.ok ? push() : claimed;
    }
    case 'forget': {
      const answer = await callHost({ type: 'forget', ...(await peer()) });
      // Set even when the app could not be reached: the user asked for this to
      // stop, and an app that is closed or gone is no reason to keep sending.
      await setOff(true);
      return answer;
    }
    case 'openApp':
      return callHost({ type: 'openApp', ...(await peer()) });
    default: {
      // Status carries nothing and changes nothing, which is what the popup
      // wants on open. Following it with a push only when this profile is the
      // bound one is what heals a link that has gone quiet.
      const status = await requestStatus();
      return status.ok && status.status?.bound ? push() : status;
    }
  }
}

chrome.runtime.onMessage.addListener((message, _sender, respond) => {
  (async () => {
    const result = await act(message?.action);
    const jar = await readJar();
    respond({
      result,
      signedIn: jar.signedIn,
      cookieCount: jar.cookies.length,
      googleGranted: await googleGranted(),
    });
  })();
  return true;
});

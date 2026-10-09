// Whose sign-in this extension can lend, and how to tell that one is there.
//
// Pure, like media.js -- no `chrome.*` -- so scripts/extension/sessions.test.mjs
// checks it without a browser. The site names are the wire's (`Site` in
// src-tauri/src/bridge/protocol.rs), and that test holds the two lists
// together: a name the host does not know makes it refuse the whole push.

import { parseHttp } from './media.js';

// For each site: the domain its cookies are read from, the cookies that carry
// a sign-in, and which changes are worth sending the jar again for.
//
// `session` is there because a jar can be full of cookies and still belong to
// a signed-out browser -- both sites set visitor and preference cookies for
// everyone -- so the presence of one of these is what `signedIn` means.
//
// `watch` is there because every push starts the app's helper. Any change to a
// youtube.com cookie is one, as it was before there was a second site. TikTok
// rewrites msToken and ttwid on most requests a page makes, which would start
// the helper every few seconds while a feed scrolls; only the sign-in's own
// cookies change when the account does, and opening the popup and the daily
// alarm carry the rest of the jar along.
export const SITES = Object.freeze({
  // YouTube keeps its own copies of the SID family on youtube.com, which is the
  // only domain read for it.
  youtube: Object.freeze({
    domain: 'youtube.com',
    session: new Set(['SID', '__Secure-1PSID', '__Secure-3PSID', 'LOGIN_INFO']),
    watch: null,
  }),
  tiktok: Object.freeze({
    domain: 'tiktok.com',
    session: new Set(['sessionid', 'sessionid_ss', 'sid_tt']),
    watch: new Set(['sessionid', 'sessionid_ss', 'sid_tt', 'sid_guard', 'uid_tt']),
  }),
});

/**
 * The site a cookie domain -- or a host -- belongs to, or null for any other.
 * By domain and subdomain, as the app's store keeps a jar: `tiktok.com.example`
 * and `tiktokv.com` are someone else's.
 */
export function siteOfDomain(domain) {
  const host = String(domain ?? '').replace(/^\./, '').toLowerCase();
  for (const [site, { domain: own }] of Object.entries(SITES)) {
    if (host === own || host.endsWith(`.${own}`)) return site;
  }
  return null;
}

/** Whether a jar read for `site` holds a sign-in, not only a visitor's cookies. */
export function signedIn(site, cookies) {
  const names = SITES[site].session;
  return cookies.some((cookie) => names.has(cookie.name) && cookie.value !== '');
}

/** Whether a change to the cookie `name` of `site`'s is a reason to push again. */
export function worthPushing(site, name) {
  const watch = SITES[site].watch;
  return watch === null || watch.has(name);
}

/**
 * The head of a push for `site`. YouTube's is a plain `push`, exactly as every
 * extension before 1.0.5 sent it. Any other site's is `pushSite`, naming the
 * site: a host from before that tag refuses it as `malformed`, where a `site`
 * field on a plain push would be ignored and the jar stored as YouTube's.
 */
export function pushHead(site) {
  return site === 'youtube' ? { type: 'push' } : { type: 'pushSite', site };
}

/**
 * The site whose session a web address could use, or null. By the address and
 * not the page it was found on, because that is how the app picks the session
 * it lends a download (`Site::for_url` in src-tauri/src/bridge/mod.rs).
 */
export function siteOfUrl(raw) {
  const url = parseHttp(raw);
  return url === null ? null : siteOfDomain(url.hostname);
}

/**
 * Whether a page is YouTube's own, where the popup offers YouTube's switch.
 * Its short links count: youtu.be is the address people share.
 */
export function onYouTube(raw) {
  const url = parseHttp(raw);
  if (url === null) return false;
  const host = url.hostname.toLowerCase();
  return siteOfDomain(host) === 'youtube' || host === 'youtu.be';
}

/** Whether a page is TikTok's own, where the popup offers TikTok's switch. */
export function onTikTok(raw) {
  return siteOfUrl(raw) === 'tiktok';
}

// The third switch, "Other sites", is not a site of SITES: it is a slot for
// whichever site the user presses İndir on, and its jar is read then and only
// then -- never on a cookie change, never by the daily alarm -- so nothing in
// SITES' loops is about it. `Site::Other` on the wire.
export const OTHER = 'other';

// Second-level labels a country code keeps under itself, so that
// `bbc.co.uk`, `trendyol.com.tr` and `example.ac.jp` are each one site rather
// than all of `co.uk`.
const SECOND_LEVELS = new Set(['co', 'com', 'net', 'org', 'gov', 'edu', 'ac', 'gen', 'bel', 'k12']);

/**
 * The registrable part of a host -- `bbc.co.uk` for `www.bbc.co.uk`,
 * `facebook.com` for `m.facebook.com` -- or null for a bare name such as
 * `localhost`, an IP address, or a public suffix on its own.
 *
 * A heuristic rather than the public suffix list, and the same one as
 * `registrable_domain` in src-tauri/src/bridge/protocol.rs, which refuses a
 * push naming anything else: the last two labels, or the last three when the
 * second-to-last is one of the second levels above under a two-letter top
 * level. A suffix it does not know makes its sites one site, which only ever
 * lends a cookie to a host that already shared the browser's jar with it.
 */
export function registrableDomain(host) {
  const name = String(host ?? '')
    .toLowerCase()
    .replace(/^\.+/, '')
    .replace(/\.+$/, '');
  if (name === '' || name.length > 253) return null;
  const labels = name.split('.');
  if (labels.length < 2) return null;
  if (!labels.every((label) => /^[a-z0-9](?:[a-z0-9-]{0,61}[a-z0-9])?$/.test(label))) return null;
  const top = labels[labels.length - 1];
  // A top level of digits is an IPv4 address.
  if (/^\d+$/.test(top)) return null;
  const second = labels[labels.length - 2];
  const keep = top.length === 2 && SECOND_LEVELS.has(second) ? 3 : 2;
  if (labels.length < keep) return null;
  return labels.slice(-keep).join('.');
}

// Registrable names whose sign-in has a switch of its own -- Google's is
// YouTube's -- and so is never lent as an other site's, under whatever
// country's suffix. The host refuses the same list.
const OWN_SWITCH = new Set([
  'youtube',
  'youtu',
  'youtube-nocookie',
  'ytimg',
  'google',
  'googlevideo',
  'tiktok',
  'tiktokv',
  'tiktokcdn',
]);

/**
 * The site whose sign-in the "Other sites" switch would lend for a page, as a
 * registrable domain, or null: for a page that is not on the web, one on an
 * IP address or a bare name like `localhost`, and one of the sites whose
 * session has a switch of its own.
 */
export function otherSiteOf(raw) {
  const url = parseHttp(raw);
  if (url === null) return null;
  // `URL` keeps an IPv6 host in its brackets, which no domain has.
  if (url.hostname.startsWith('[')) return null;
  const domain = registrableDomain(url.hostname);
  if (domain === null) return null;
  return OWN_SWITCH.has(domain.split('.')[0]) ? null : domain;
}

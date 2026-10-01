// What counts as a video on a page, and what the popup should list.
//
// Everything here is pure: no chrome.* call, no DOM, no clock. The service
// worker feeds it what webRequest and the page told it, the popup only formats
// what comes back, and scripts/extension/media.test.mjs runs it under plain
// Node. Keeping the judgement calls in one testable place is the point -- a
// heuristic about segment names that lives inside an event listener is one
// nobody can check without a browser and a site that happens to misbehave.
//
// Nothing in this file decides whether something may be downloaded. It sorts
// what the browser already fetched into rows; the app makes every real
// decision again, with the page in front of it.

export const MAX_ITEMS = 60;
export const MAX_ROWS = 30;

// A video file under this size is a thumbnail preview, a hover loop or a
// player's silent probe, never the thing the user came for.
const MIN_VIDEO_BYTES = 300 * 1024;
const MIN_AUDIO_BYTES = 64 * 1024;

// Contract A in the spec: the fields a `download` request carries besides the
// Peer. The host rejects anything longer than these limits as malformed, so a
// field that does not fit is left out here rather than sinking the request.
const URL_LIMIT = 8192;
const TITLE_LIMIT = 300;
const AGENT_LIMIT = 512;

export const PAYLOAD_KINDS = new Set(['page', 'stream', 'video', 'audio']);
export const PAYLOAD_FIELDS = [
  'url',
  'kind',
  'title',
  'pageUrl',
  'referer',
  'origin',
  'userAgent',
  'thumbnail',
];

const HLS_TYPES = new Set([
  'application/vnd.apple.mpegurl',
  'application/x-mpegurl',
  'application/mpegurl',
  'audio/mpegurl',
  'audio/x-mpegurl',
]);
const DASH_TYPES = new Set(['application/dash+xml']);
const SEGMENT_TYPES = new Set(['video/mp2t', 'video/iso.segment']);
const OCTET_TYPES = new Set(['application/octet-stream', 'binary/octet-stream']);

const VIDEO_EXTS = new Set([
  'mp4', 'm4v', 'webm', 'mkv', 'mov', 'avi', 'flv', 'f4v', 'wmv', 'mpg', 'mpeg', '3gp', 'ogv',
]);
const AUDIO_EXTS = new Set(['mp3', 'm4a', 'aac', 'wav', 'opus', 'ogg', 'oga', 'flac', 'weba', 'wma']);
const SEGMENT_EXTS = new Set(['ts', 'm4s', 'm4f', 'cmfv', 'cmfa']);

// A player streaming in pieces names them like this. Anchored to the start of
// the name on purpose: `lecture-part3.mp4` is somebody's file, `part3.mp4` is
// the third slice of one.
const SEGMENT_NAME = /^(?:seg|segment|chunk|frag|fragment|part)[-_]?\d+(?:[-_.].*)?$/i;
const INIT_NAME = /^init(?:[-_.].*)?$/i;

// Hosts whose media is advertising. Their video is never what the user is
// watching, and listing it would put an advert at the top of the popup more
// often than not, since it is what plays first.
const AD_HOSTS = [
  'doubleclick.net',
  'googlesyndication.com',
  'imasdk.googleapis.com',
  'googleadservices.com',
  '2mdn.net',
  'adnxs.com',
  'amazon-adsystem.com',
  'moatads.com',
  'adsafeprotected.com',
  'innovid.com',
  'spotxchange.com',
  'springserve.com',
  'serving-sys.com',
  'flashtalking.com',
  'fwmrm.net',
  'teads.tv',
  'adform.net',
  'smartadserver.com',
  'tremorhub.com',
  'unrulymedia.com',
  'pubmatic.com',
  'rubiconproject.com',
];

// Sites the app reads by their page address rather than by a file: a link to
// the page gets the right title, every quality and the audio that belongs with
// the picture, where the files the player fetched would get a nameless slice.
// The more specific host comes first so YouTube Music is not called YouTube.
export const PAGE_SITES = [
  ['music.youtube.com', 'YouTube Music'],
  ['youtube.com', 'YouTube'],
  ['youtu.be', 'YouTube'],
  ['facebook.com', 'Facebook'],
  ['fb.watch', 'Facebook'],
  ['instagram.com', 'Instagram'],
  ['threads.net', 'Threads'],
  ['tiktok.com', 'TikTok'],
  ['x.com', 'X'],
  ['twitter.com', 'X'],
  ['vimeo.com', 'Vimeo'],
  ['twitch.tv', 'Twitch'],
  ['kick.com', 'Kick'],
  ['dailymotion.com', 'Dailymotion'],
  ['reddit.com', 'Reddit'],
  ['soundcloud.com', 'SoundCloud'],
  ['bandcamp.com', 'Bandcamp'],
  ['bilibili.com', 'Bilibili'],
  ['rumble.com', 'Rumble'],
  ['pinterest.com', 'Pinterest'],
  ['linkedin.com', 'LinkedIn'],
  ['open.spotify.com', 'Spotify'],
];

// Sites the popup lists nothing for, not even a stream one of their players
// fetched. Empty: it is the lever to pull if store review ever insists that
// YouTube go (its three hosts go here), and the only one. Taking YouTube out
// of PAGE_SITES alone would not do it, because the page fallback in `rows`
// offers any page with a <video> on it, and YouTube's is one.
export const UNLISTED_SITES = [];

// A mirror of PROTECTED in src-tauri/src/providers/detect.rs, kept in the same
// order so the two can be compared line by line. The app refuses these by name;
// the popup says so before the user presses anything, rather than offering a
// button whose only outcome is that refusal.
export const PROTECTED = [
  ['netflix.com', 'Netflix'],
  ['disneyplus.com', 'Disney+'],
  ['primevideo.com', 'Prime Video'],
  ['max.com', 'Max'],
  ['hbomax.com', 'HBO Max'],
  ['hulu.com', 'Hulu'],
  ['paramountplus.com', 'Paramount+'],
  ['peacocktv.com', 'Peacock'],
  ['tv.apple.com', 'Apple TV+'],
  ['music.apple.com', 'Apple Music'],
  ['crunchyroll.com', 'Crunchyroll'],
  ['blutv.com', 'BluTV'],
  ['exxen.com', 'Exxen'],
  ['gain.tv', 'Gain'],
  ['tod.tv', 'TOD'],
  ['tvplus.com.tr', 'TV+'],
  ['deezer.com', 'Deezer'],
  ['tidal.com', 'TIDAL'],
];

// Key systems by the names an HLS key line uses for them. Plain AES-128 is not
// here: it is a key fetched over HTTPS that yt-dlp and ffmpeg apply themselves,
// and calling it protected would hide half the streams on the web.
const DRM_MARKERS = [
  'edef8ba9-79d6-4ace-a3c8-27dcd51d21ed', // Widevine
  'widevine',
  'com.microsoft.playready',
  '9a04f079-9840-4286-ab92-e65be0885f95', // PlayReady
  'playready',
  'com.apple.streamingkeydelivery', // FairPlay
  'com.apple.fps',
  'skd://',
];
const DRM_METHODS = new Set(['SAMPLE-AES', 'SAMPLE-AES-CTR', 'SAMPLE-AES-CENC', 'ISO-23001-7']);

// Query parameters that pick a byte range out of a file. Two requests for two
// ranges of one file are the same file, and handing the app a URL that still
// says `range=0-1000` would download a kilobyte.
const RANGE_PARAMS = ['range', 'bytestart', 'byteend'];

/** The URL parsed, when it is http or https; null for anything else. */
export function parseHttp(raw) {
  if (typeof raw !== 'string' || raw === '') return null;
  try {
    const url = new URL(raw);
    return url.protocol === 'http:' || url.protocol === 'https:' ? url : null;
  } catch {
    return null;
  }
}

export function isHttpUrl(raw) {
  return parseHttp(raw) !== null;
}

function onHost(host, suffix) {
  return host === suffix || host.endsWith(`.${suffix}`);
}

/** The host as a person would say it: no `www.`, nothing for a non-web page. */
export function displayHost(raw) {
  const url = parseHttp(raw);
  return url ? url.hostname.replace(/^www\./, '') : '';
}

/**
 * The URL a row stands for, and the key two sightings of it are compared by:
 * no fragment, no byte-range parameters.
 */
export function dedupeKey(raw) {
  const url = parseHttp(raw);
  if (!url) return '';
  url.hash = '';
  for (const name of RANGE_PARAMS) url.searchParams.delete(name);
  return url.href;
}

function withoutHash(raw) {
  const at = typeof raw === 'string' ? raw.indexOf('#') : -1;
  return at === -1 ? (raw ?? '') : raw.slice(0, at);
}

/** Whether two tab URLs are the same page, ignoring an in-page anchor. */
export function samePage(a, b) {
  return withoutHash(a) === withoutHash(b);
}

function originOf(raw) {
  const url = parseHttp(raw);
  return url ? url.origin : '';
}

/** The last path segment, lower-cased, split into stem and extension. */
function fileOf(url) {
  const segment = url.pathname.split('/').pop() ?? '';
  let name = segment;
  try {
    name = decodeURIComponent(segment);
  } catch {
    // A stray `%` in a name is still a name.
  }
  name = name.toLowerCase();
  const dot = name.lastIndexOf('.');
  if (dot <= 0) return { stem: name, ext: '' };
  return { stem: name.slice(0, dot), ext: name.slice(dot + 1) };
}

/**
 * Whether a request is media that should never become a row of its own: a
 * slice of something bigger, or an advert. It still tells the worker that the
 * tab is playing something, which is what shows the page row on YouTube, where
 * every byte of the video arrives as one of these.
 */
export function isNoise(raw) {
  const url = parseHttp(raw);
  if (!url) return false;
  const host = url.hostname.toLowerCase();
  const path = url.pathname.toLowerCase();

  if (onHost(host, 'googlevideo.com') && path.includes('videoplayback')) return true;
  if (RANGE_PARAMS.some((name) => url.searchParams.has(name))) return true;
  if (host.includes('vimeo') && path.includes('/range/')) return true;
  if (AD_HOSTS.some((suffix) => onHost(host, suffix))) return true;
  return /\/(?:ads|vast)\//.test(path);
}

function mimeOf(contentType) {
  return String(contentType ?? '').split(';')[0].trim().toLowerCase();
}

function kindOf(url, mime, type) {
  const { stem, ext } = fileOf(url);
  const path = url.pathname.toLowerCase();

  if (HLS_TYPES.has(mime) || ext === 'm3u8') return 'hls';
  // Smooth Streaming manifests end in `.ism/Manifest`; yt-dlp reads them with
  // the same machinery as DASH, so they are listed as one.
  if (DASH_TYPES.has(mime) || ext === 'mpd' || /\.isml?\/manifest$/.test(path)) return 'dash';

  if (SEGMENT_TYPES.has(mime) || SEGMENT_EXTS.has(ext)) return 'segment';
  const mediaExt = VIDEO_EXTS.has(ext) || AUDIO_EXTS.has(ext);
  if (mediaExt && (SEGMENT_NAME.test(stem) || INIT_NAME.test(stem))) return 'segment';
  // `fileSequence12.aac` and `000123.mp4`. A bare number loaded straight into
  // a media element is a whole file that happens to be named by its id;
  // fetched by script, it is a slice.
  if (ext === 'aac' && /\d$/.test(stem)) return 'segment';
  if (mediaExt && /^\d+$/.test(stem) && type !== 'media') return 'segment';

  // A file extension only counts when the server did not say it is something
  // else: `video.mp4` answered with text/html is a landing page.
  const extTrusted = mime === '' || OCTET_TYPES.has(mime) || mime.startsWith('video/') || mime.startsWith('audio/');

  if (mime.startsWith('video/') || (VIDEO_EXTS.has(ext) && extTrusted)) return 'video';
  if (mime.startsWith('audio/') || (AUDIO_EXTS.has(ext) && extTrusted)) return 'audio';

  // A media element pointed at a nameless URL that the server will only call
  // bytes. The element loading it is the evidence the name would have been.
  if (type === 'media' && (mime === '' || OCTET_TYPES.has(mime))) return 'video';
  return null;
}

/**
 * What a response is: 'hls', 'dash', 'video' or 'audio' for something worth a
 * row; 'segment' or 'noise' for media that is not; null for everything else.
 *
 * `type` is the webRequest resource type ('media', 'xmlhttprequest', ...).
 */
export function classify({ url, contentType = '', type = '' } = {}) {
  const parsed = parseHttp(url);
  if (!parsed) return null;

  // YouTube's player fetches its media as an opaque byte stream with a type of
  // its own, so this one host has to be recognised before the type is read.
  const host = parsed.hostname.toLowerCase();
  if (onHost(host, 'googlevideo.com') && parsed.pathname.includes('videoplayback')) return 'noise';

  const kind = kindOf(parsed, mimeOf(contentType), type);
  if (!kind) return null;
  if (kind !== 'segment' && isNoise(url)) return 'noise';
  return kind;
}

/** webRequest's header list as a plain object keyed by lower-case name. */
export function headerMap(list) {
  const out = {};
  if (!Array.isArray(list)) return out;
  for (const header of list) {
    if (!header || typeof header.name !== 'string') continue;
    out[header.name.toLowerCase()] = typeof header.value === 'string' ? header.value : '';
  }
  return out;
}

/**
 * The whole file's size in bytes, when the response says it.
 *
 * A player asks for a file a range at a time, and the length of a 206 is the
 * length of the range. `Content-Range` carries the total after the slash; a
 * 206 without one says nothing about the file.
 */
export function sizeFrom(headers, statusCode) {
  const range = headers?.['content-range'];
  if (typeof range === 'string') {
    const total = /\/\s*(\d+)\s*$/.exec(range);
    if (total) return Number(total[1]);
  }
  if (statusCode === 206) return null;
  const length = headers?.['content-length'];
  if (typeof length === 'string' && /^\s*\d+\s*$/.test(length)) return Number(length);
  return null;
}

/** Whether a file is too small to be what anyone is watching or hearing. */
export function tooSmall(kind, size) {
  if (typeof size !== 'number') return false;
  if (kind === 'video') return size < MIN_VIDEO_BYTES;
  if (kind === 'audio') return size < MIN_AUDIO_BYTES;
  return false;
}

function resolve(uri, base) {
  try {
    const url = new URL(uri, base);
    return url.protocol === 'http:' || url.protocol === 'https:' ? url.href : null;
  } catch {
    return null;
  }
}

// `KEY=VALUE,KEY="quoted, with commas"`, the attribute list every HLS tag uses.
function attributes(line) {
  const out = {};
  const list = line.slice(line.indexOf(':') + 1);
  const pattern = /([A-Z0-9-]+)=("([^"]*)"|[^,]*)/g;
  let match;
  while ((match = pattern.exec(list)) !== null) {
    out[match[1]] = match[3] ?? match[2];
  }
  return out;
}

function drmKey(attrs) {
  if (DRM_METHODS.has(String(attrs.METHOD ?? '').toUpperCase())) return true;
  const named = `${attrs.KEYFORMAT ?? ''} ${attrs.URI ?? ''}`.toLowerCase();
  return DRM_MARKERS.some((marker) => named.includes(marker));
}

/**
 * An HLS playlist, read for what the popup shows.
 *
 * A master playlist gives its variants (as absolute URLs, so the worker can
 * hide the ones the player went on to fetch), its tallest resolution, and
 * whether any session key names a DRM system. A media playlist gives its
 * length, when it has one: a live playlist without `#EXT-X-ENDLIST` is a
 * window onto the stream, and the sum of its segments is not a duration.
 */
export function parseHls(text, baseUrl) {
  if (typeof text !== 'string') return null;
  const lines = text
    .replace(/^﻿/, '')
    .split(/\r?\n/)
    .map((line) => line.trim())
    .filter(Boolean);
  if (!lines[0]?.startsWith('#EXTM3U')) return null;

  let master = false;
  let height = 0;
  let seconds = 0;
  let ended = false;
  let isProtected = false;
  let expectUri = false;
  const variants = [];

  for (const line of lines) {
    if (line.startsWith('#EXT-X-STREAM-INF:')) {
      master = true;
      const found = /x(\d+)/i.exec(attributes(line).RESOLUTION ?? '');
      if (found) height = Math.max(height, Number(found[1]));
      expectUri = true;
    } else if (line.startsWith('#EXT-X-I-FRAME-STREAM-INF:') || line.startsWith('#EXT-X-MEDIA:')) {
      if (line.startsWith('#EXT-X-I-FRAME')) master = true;
      const uri = attributes(line).URI;
      if (uri) variants.push(resolve(uri, baseUrl));
    } else if (line.startsWith('#EXT-X-KEY:') || line.startsWith('#EXT-X-SESSION-KEY:')) {
      if (drmKey(attributes(line))) isProtected = true;
    } else if (line.startsWith('#EXTINF:')) {
      const value = Number.parseFloat(line.slice('#EXTINF:'.length));
      if (Number.isFinite(value) && value > 0) seconds += value;
    } else if (line.startsWith('#EXT-X-ENDLIST') || line === '#EXT-X-PLAYLIST-TYPE:VOD') {
      ended = true;
    } else if (!line.startsWith('#') && expectUri) {
      variants.push(resolve(line, baseUrl));
      expectUri = false;
    }
  }

  if (master) {
    return {
      master: true,
      variants: [...new Set(variants.filter(Boolean))],
      height: height || null,
      protected: isProtected,
    };
  }
  return {
    master: false,
    durationSec: ended && seconds > 0 ? Math.round(seconds * 1000) / 1000 : null,
    protected: isProtected,
  };
}

/** `PT1H2M3.5S` -> 3723.5. Null for anything that is not a positive duration. */
export function isoDuration(value) {
  const match =
    /^P(?:(\d+)Y)?(?:(\d+)M)?(?:(\d+)W)?(?:(\d+)D)?(?:T(?:(\d+(?:\.\d+)?)H)?(?:(\d+(?:\.\d+)?)M)?(?:(\d+(?:\.\d+)?)S)?)?$/.exec(
      String(value ?? '').trim(),
    );
  if (!match) return null;
  const [years, months, weeks, days, hours, minutes, seconds] = match
    .slice(1)
    .map((part) => Number(part ?? 0));
  // Years and months have no fixed length, and no manifest uses them for a
  // video's running time; the nominal values keep a strange one from failing.
  const total =
    ((years * 365 + months * 30 + weeks * 7 + days) * 24 + hours) * 3600 + minutes * 60 + seconds;
  return total > 0 ? Math.round(total * 1000) / 1000 : null;
}

function xmlAttr(tag, name) {
  const match = new RegExp(`\\s${name}\\s*=\\s*"([^"]*)"`, 'i').exec(tag);
  return match ? match[1] : null;
}

function xmlText(value) {
  return value
    .replace(/&amp;/g, '&')
    .replace(/&lt;/g, '<')
    .replace(/&gt;/g, '>')
    .replace(/&quot;/g, '"')
    .replace(/&apos;/g, "'")
    .trim();
}

// Smooth Streaming: the same three facts, in Microsoft's spelling. Duration is
// in TimeScale ticks, which default to ten million a second.
function parseSmooth(text) {
  const root = /<SmoothStreamingMedia\b[^>]*>/i.exec(text)?.[0] ?? '';
  const ticks = Number(xmlAttr(root, 'Duration'));
  const scale = Number(xmlAttr(root, 'TimeScale')) || 10_000_000;
  const live = String(xmlAttr(root, 'IsLive')).toLowerCase() === 'true';
  let height = 0;
  for (const [tag] of text.matchAll(/<QualityLevel\b[^>]*>/gi)) {
    height = Math.max(height, Number(xmlAttr(tag, 'MaxHeight')) || 0);
  }
  return {
    master: true,
    variants: [],
    height: height || null,
    durationSec: !live && ticks > 0 ? Math.round((ticks / scale) * 1000) / 1000 : null,
    protected: /<Protection(?:Header)?\b/i.test(text),
  };
}

/**
 * A DASH manifest, read with patterns rather than a parser: a service worker
 * has no DOMParser, and three facts do not justify carrying one.
 *
 * `variants` are the BaseURLs it names. A player reading a SegmentBase
 * manifest fetches those files whole, by range, and without this they would
 * each appear as a video with no sound and a sound with no video beside the
 * stream they belong to.
 */
export function parseDash(text, baseUrl) {
  if (typeof text !== 'string') return null;
  if (/<SmoothStreamingMedia\b/i.test(text)) return parseSmooth(text);
  const root = /<(?:\w+:)?MPD\b[^>]*>/.exec(text)?.[0];
  if (!root) return null;

  let height = 0;
  for (const [tag] of text.matchAll(/<(?:\w+:)?Representation\b[^>]*>/g)) {
    height = Math.max(height, Number(xmlAttr(tag, 'height')) || 0);
  }
  if (!height) {
    for (const [tag] of text.matchAll(/<(?:\w+:)?AdaptationSet\b[^>]*>/g)) {
      height = Math.max(height, Number(xmlAttr(tag, 'maxHeight') ?? xmlAttr(tag, 'height')) || 0);
    }
  }

  const live = String(xmlAttr(root, 'type')).toLowerCase() === 'dynamic';
  const bases = [...text.matchAll(/<(?:\w+:)?BaseURL\b[^>]*>([^<]*)</g)].map((m) => xmlText(m[1]));
  // A BaseURL is relative to the one above it. Resolving each against the
  // manifest and against the first one covers the shapes seen in practice;
  // a wrong extra entry only hides nothing.
  const root0 = bases[0] ? resolve(bases[0], baseUrl) : null;
  const variants = new Set();
  for (const base of bases) {
    const direct = resolve(base, baseUrl);
    if (direct) variants.add(direct);
    const nested = root0 ? resolve(base, root0) : null;
    if (nested) variants.add(nested);
  }

  return {
    master: true,
    variants: [...variants],
    height: height || null,
    durationSec: live ? null : isoDuration(xmlAttr(root, 'mediaPresentationDuration')),
    protected: /<(?:\w+:)?ContentProtection\b/.test(text),
  };
}

/** The display name of a site the app reads by page address, or null. */
export function siteOf(raw) {
  const url = parseHttp(raw);
  if (!url) return null;
  const host = url.hostname.toLowerCase();
  const found = PAGE_SITES.find(([suffix]) => onHost(host, suffix));
  if (found) return found[1];
  // Pinterest's country domains, as detect.rs matches them.
  if (host.replace(/^www\./, '').startsWith('pinterest.')) return 'Pinterest';
  return null;
}

/** The protected service a page belongs to, mirroring detect.rs, or null. */
export function protectedService(raw) {
  const url = parseHttp(raw);
  if (!url) return null;
  const host = url.hostname.toLowerCase();
  if (host.split('.').includes('amazon') && url.pathname.startsWith('/gp/video')) {
    return 'Prime Video';
  }
  const found = PROTECTED.find(([suffix]) => onHost(host, suffix));
  return found ? found[1] : null;
}

function unlisted(raw) {
  const url = parseHttp(raw);
  if (!url) return false;
  const host = url.hostname.toLowerCase();
  return UNLISTED_SITES.some((suffix) => onHost(host, suffix));
}

// YouTube does not update its og: tags when it moves from one video to the
// next without a page load, so on the page people use most the tags usually
// describe the first video of the visit. The thumbnail can be named from the
// address instead.
function youtubeThumbnail(url) {
  const host = url.hostname.toLowerCase();
  let id = null;
  if (onHost(host, 'youtu.be')) id = url.pathname.slice(1).split('/')[0];
  else if (onHost(host, 'youtube.com')) {
    id = url.searchParams.get('v') ?? /^\/(?:shorts|live|embed)\/([\w-]+)/.exec(url.pathname)?.[1] ?? null;
  }
  return id && /^[\w-]{6,20}$/.test(id) ? `https://i.ytimg.com/vi/${id}/mqdefault.jpg` : null;
}

/**
 * A title fit for the wire: control characters gone, whitespace folded, a
 * leading notification count dropped (`(3) Song name`), at most 300
 * characters. Page titles are whatever the page says, so this runs on every
 * one of them.
 */
export function cleanTitle(value) {
  const text = String(value ?? '')
    .replace(/[\u0000-\u001f\u007f-\u009f]/g, ' ')
    .replace(/\s+/g, ' ')
    .trim()
    .replace(/^\(\d+\+?\)\s+/, '');
  const chars = Array.from(text);
  return chars.length > TITLE_LIMIT ? chars.slice(0, TITLE_LIMIT).join('').trim() : text;
}

function stripSiteName(title, site) {
  if (!site) return title;
  const escaped = site.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
  return title.replace(new RegExp(`\\s+[-|•·–—/]\\s+${escaped}$`, 'i'), '').trim();
}

// Whether the page's og: tags still describe the page. They do when there is
// no og:url to say otherwise, or when og:url names this path and every one of
// its parameters is still in the address -- `watch?v=A` against
// `watch?v=A&t=30` is the same video, against `watch?v=B` it is not.
function ogFresh(top, pageUrl) {
  if (!top?.ogUrl) return true;
  const og = parseHttp(top.ogUrl);
  const page = parseHttp(pageUrl);
  if (!og || !page) return false;
  if (og.pathname.replace(/\/$/, '') !== page.pathname.replace(/\/$/, '')) return false;
  for (const [name, value] of og.searchParams) {
    if (page.searchParams.get(name) !== value) return false;
  }
  return true;
}

/** The Referer the browser would have sent, near enough. */
export function refererFor(initiator, pageUrl) {
  const page = parseHttp(pageUrl);
  const from = parseHttp(initiator);
  if (!from || from.origin !== initiator) return page ? page.href : null;
  if (page && from.origin === page.origin) return page.href;
  return `${from.origin}/`;
}

/** The Origin header a script's request carried, or null for any other. */
export function originFor(initiator, isXhr) {
  if (!isXhr) return null;
  const from = parseHttp(initiator);
  return from && from.origin === initiator ? from.origin : null;
}

function fits(value, limit) {
  return typeof value === 'string' && value !== '' && new TextEncoder().encode(value).length <= limit;
}

/**
 * Contract A's fields, from whatever the popup sent back. Unknown fields are
 * dropped, an unknown kind is a page, and a value the host would refuse for
 * its length is left out instead.
 */
export function toPayload(raw) {
  if (!raw || typeof raw !== 'object') return null;
  if (!isHttpUrl(raw.url) || !fits(raw.url, URL_LIMIT)) return null;
  const web = (value) => (isHttpUrl(value) && fits(value, URL_LIMIT) ? value : undefined);
  const origin = parseHttp(raw.origin);
  const agentOk = fits(raw.userAgent, AGENT_LIMIT) && !/[\u0000-\u001f\u007f]/.test(raw.userAgent);

  // Built in the contract's order, so what goes over the wire reads like the
  // spec it was written from.
  const out = {
    url: raw.url,
    kind: PAYLOAD_KINDS.has(raw.kind) ? raw.kind : 'page',
    title: cleanTitle(raw.title) || undefined,
    pageUrl: web(raw.pageUrl),
    referer: web(raw.referer),
    origin: origin && origin.origin === raw.origin ? raw.origin : undefined,
    userAgent: agentOk ? raw.userAgent : undefined,
    thumbnail: web(raw.thumbnail),
  };
  for (const field of PAYLOAD_FIELDS) {
    if (out[field] === undefined) delete out[field];
  }
  return out;
}

function num(value) {
  return typeof value === 'number' && Number.isFinite(value) && value > 0 ? value : 0;
}

function str(value, limit = URL_LIMIT) {
  return typeof value === 'string' && value.length <= limit ? value : '';
}

function element(raw) {
  return {
    src: isHttpUrl(raw?.src) ? str(raw.src) : '',
    poster: isHttpUrl(raw?.poster) ? str(raw.poster) : '',
    duration: num(raw?.duration),
    width: num(raw?.width),
    height: num(raw?.height),
    drm: raw?.drm === true,
  };
}

/**
 * One frame's report from the page, made safe to reason about. The script that
 * produced it ran in the page, so every field is checked for type and size
 * here rather than trusted.
 */
export function normaliseFrame(raw, frameId) {
  const list = (value) => (Array.isArray(value) ? value.slice(0, 50).map(element) : []);
  return {
    frameId: Number.isInteger(frameId) ? frameId : 0,
    href: str(raw?.href),
    isTop: raw?.isTop === true,
    title: str(raw?.title, 2000),
    ogTitle: str(raw?.ogTitle, 2000),
    ogImage: isHttpUrl(raw?.ogImage) ? str(raw.ogImage) : '',
    ogUrl: str(raw?.ogUrl),
    videos: list(raw?.videos),
    audios: list(raw?.audios),
  };
}

const STREAM_KINDS = new Set(['hls', 'dash']);

function quality(height) {
  return height ? `${height}p` : null;
}

function metaParts({ height, size, durationSec }) {
  const parts = [];
  const tall = quality(height);
  if (tall) parts.push(tall);
  else if (size) parts.push(formatSize(size));
  if (durationSec) parts.push(formatDuration(durationSec));
  return parts;
}

// The running time of a frame's only video. A stream's master playlist does
// not say how long it is, but the one player on the page knows, and when there
// is exactly one there is no doubt which video it is talking about.
function soleDuration(frame) {
  if (!frame || frame.videos.length !== 1) return null;
  return frame.videos[0].duration || null;
}

function rank(row) {
  if (row.kind === 'page') return 0;
  if (row.protected) return 4;
  return { stream: 1, video: 2, audio: 3 }[row.kind] ?? 3;
}

function compareRows(a, b) {
  const byRank = rank(a) - rank(b);
  if (byRank !== 0) return byRank;
  if (a.kind === 'stream') return (b.height ?? 0) - (a.height ?? 0);
  return (b.size ?? 0) - (a.size ?? 0) || (b.height ?? 0) - (a.height ?? 0);
}

/**
 * The popup's list for one tab.
 *
 * `state` is the worker's record of the tab (`url`, `title`, `sawMedia`,
 * `items`), `reported` what each frame of the page said when the popup
 * opened -- empty when the badge is being counted, which is why a page row
 * that only an element can justify is missing from the badge until the popup
 * has been opened once.
 */
export function rows(state, reported = [], { userAgent = '' } = {}) {
  const pageUrl = typeof state?.url === 'string' ? state.url : '';
  if (protectedService(pageUrl) || unlisted(pageUrl)) return [];
  // A player embedded from an unlisted site counts for nothing either.
  const frames = reported.filter((frame) => !unlisted(frame.href));

  const page = parseHttp(pageUrl);
  const site = siteOf(pageUrl);
  const top = frames.find((frame) => frame.isTop) ?? null;
  const fresh = ogFresh(top, pageUrl);
  const items = Array.isArray(state?.items) ? state.items : [];

  const rawTitle = (fresh && top?.ogTitle) || top?.title || state?.title || '';
  const pageTitle = cleanTitle(stripSiteName(cleanTitle(rawTitle), site)) || displayHost(pageUrl);
  const ogImage = fresh && isHttpUrl(top?.ogImage) ? top.ogImage : '';
  const agent = typeof userAgent === 'string' ? userAgent : '';

  const drmFrames = new Set(
    frames
      .filter((frame) => [...frame.videos, ...frame.audios].some((media) => media.drm))
      .map((frame) => frame.frameId),
  );
  const frameById = (frameId) => frames.find((frame) => frame.frameId === frameId) ?? null;
  const posterIn = (frameId) => frameById(frameId)?.videos.find((video) => video.poster)?.poster ?? '';

  // Every URL a parsed manifest names is part of that manifest's row.
  const hidden = new Set();
  for (const item of items) {
    for (const variant of item.info?.variants ?? []) hidden.add(dedupeKey(variant));
  }

  // Elements by the URL they play, so a file both fetched and on screen
  // becomes one row with the element's resolution and length.
  const elements = new Map();
  for (const frame of frames) {
    for (const [tag, list] of [['video', frame.videos], ['audio', frame.audios]]) {
      for (const media of list) {
        const key = dedupeKey(media.src);
        if (key && !elements.has(key)) elements.set(key, { ...media, tag, frame });
      }
    }
  }

  const out = [];
  const seen = new Set();

  for (const item of items) {
    const key = dedupeKey(item.url);
    if (!key || hidden.has(key) || seen.has(key) || unlisted(item.initiator)) continue;
    seen.add(key);
    const shown = elements.get(key);
    const stream = STREAM_KINDS.has(item.kind);
    const kind = stream ? 'stream' : item.kind === 'audio' ? 'audio' : 'video';
    const height = item.info?.height ?? (shown?.height || null);
    const durationSec =
      item.info?.durationSec ??
      (shown?.duration || (stream ? soleDuration(frameById(item.frameId)) : null));
    const thumbnail = shown?.poster || posterIn(item.frameId) || ogImage;
    const payload = toPayload({
      url: key,
      kind,
      title: pageTitle,
      pageUrl,
      referer: refererFor(item.initiator, pageUrl),
      origin: originFor(item.initiator, item.isXhr),
      userAgent: agent,
      thumbnail,
    });
    if (!payload) continue;
    out.push({
      key: `item:${item.id ?? key}`,
      kind,
      url: key,
      title: pageTitle,
      thumbnail,
      protected: Boolean(item.info?.protected) || drmFrames.has(item.frameId) || shown?.drm === true,
      height: height || null,
      size: typeof item.size === 'number' ? item.size : null,
      durationSec: durationSec || null,
      payload,
    });
  }

  for (const [key, media] of elements) {
    if (seen.has(key) || hidden.has(key)) continue;
    const found = classify({ url: media.src, type: 'media' });
    if (found === 'segment' || found === 'noise') continue;
    seen.add(key);
    const kind = STREAM_KINDS.has(found) ? 'stream' : media.tag;
    const initiator = originOf(media.frame.href);
    const thumbnail = media.poster || ogImage;
    const payload = toPayload({
      url: key,
      kind,
      title: pageTitle,
      pageUrl,
      referer: refererFor(initiator, pageUrl),
      userAgent: agent,
      thumbnail,
    });
    if (!payload) continue;
    out.push({
      key: `element:${key}`,
      kind,
      url: key,
      title: pageTitle,
      thumbnail,
      protected: media.drm || drmFrames.has(media.frame.frameId),
      height: media.height || null,
      size: null,
      durationSec: media.duration || null,
      payload,
    });
  }

  // The page itself. On a site the app knows, the page address is the better
  // thing to hand over whenever something is playing -- but never a site's
  // front page, which is a feed with a preview playing rather than a video.
  // Elsewhere it is the last resort for a player that builds its video from
  // script (a blob: source), where nothing else could be listed -- and when a
  // player on the page uses EME, that player is the one it stands for, so the
  // row says it is protected instead of offering a button that can only fail.
  const hasElements = frames.some((frame) => frame.videos.length > 0 || frame.audios.length > 0);
  let pageRow = null;
  const content = page && page.pathname !== '/' && page.pathname !== '';
  if (site && content && (state?.sawMedia || items.length > 0 || hasElements)) {
    pageRow = { label: 'site', site };
  } else if (!site && page && out.length === 0 && hasElements) {
    pageRow = { label: drmFrames.size > 0 ? 'protected' : 'page' };
  }

  out.sort(compareRows);

  const list = out.map((row) => ({
    ...row,
    meta: row.protected
      ? { label: 'protected', parts: [] }
      : { label: row.kind, parts: metaParts(row) },
  }));

  if (pageRow) {
    const thumbnail = ogImage || (page ? youtubeThumbnail(page) : null) || posterIn(top?.frameId ?? 0) || '';
    const payload = toPayload({ url: pageUrl, kind: 'page', title: pageTitle, pageUrl, userAgent: agent, thumbnail });
    if (payload) {
      const locked = pageRow.label === 'protected';
      list.unshift({
        key: 'page',
        kind: 'page',
        url: pageUrl,
        title: pageTitle,
        thumbnail,
        protected: locked,
        height: null,
        size: null,
        durationSec: soleDuration(top),
        meta: { ...pageRow, parts: locked ? [] : metaParts({ durationSec: soleDuration(top) }) },
        payload,
      });
    }
  }

  return list.slice(0, MAX_ROWS);
}

const UNITS = ['B', 'KB', 'MB', 'GB', 'TB'];

/** 25 165 824 -> "24 MB". Binary units, like the app's formatBytes. */
export function formatSize(bytes) {
  if (typeof bytes !== 'number' || !Number.isFinite(bytes) || bytes < 0) return '';
  let value = bytes;
  let unit = 0;
  while (value >= 1024 && unit < UNITS.length - 1) {
    value /= 1024;
    unit += 1;
  }
  const digits = unit <= 1 || value >= 10 ? 0 : 1;
  return `${value.toFixed(digits)} ${UNITS[unit]}`;
}

/** Seconds -> "4:05" or "1:02:03", like the app's formatDuration. */
export function formatDuration(totalSeconds) {
  if (typeof totalSeconds !== 'number' || !Number.isFinite(totalSeconds) || totalSeconds <= 0) {
    return '';
  }
  const whole = Math.round(totalSeconds);
  const seconds = whole % 60;
  const minutes = Math.floor(whole / 60) % 60;
  const hours = Math.floor(whole / 3600);
  const ss = String(seconds).padStart(2, '0');
  return hours > 0 ? `${hours}:${String(minutes).padStart(2, '0')}:${ss}` : `${minutes}:${ss}`;
}

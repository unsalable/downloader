// Unit tests for extension/sessions.js: whose sign-in the extension lends, and
// how it tells one is there.
//
//   node --test scripts/extension/
//
// Like media.test.mjs, this lives outside extension/ so the store package
// never carries it, and takes its `describe`/`test` from whichever runner
// loaded it -- Vitest's default pattern picks up every *.test.mjs as well.
//
// Three of these read the app's own source rather than a copy of it: the site
// names the host accepts, the cookie names the app's log masks, and the two
// languages the popup speaks. Each is a list kept in step by hand, and each
// goes wrong without a sound -- a push the host refuses whole, a session value
// written to a log, a switch with no title.

import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

import {
  OTHER,
  SITES,
  onTikTok,
  onYouTube,
  otherSiteOf,
  pushHead,
  registrableDomain,
  signedIn,
  siteOfDomain,
  siteOfUrl,
  worthPushing,
} from '../../extension/sessions.js';

const { describe, test } = process.env.VITEST ? await import('vitest') : await import('node:test');

const repo = join(dirname(fileURLToPath(import.meta.url)), '..', '..');
const read = (...parts) => readFileSync(join(repo, ...parts), 'utf8');

const cookie = (name, value = 'v') => ({ domain: '.example', name, value });

describe('sessions', () => {
  test('a cookie belongs to the site whose domain it is under', () => {
    for (const domain of ['.tiktok.com', 'tiktok.com', 'www.tiktok.com', 'WWW.TikTok.com', '.m.tiktok.com']) {
      assert.equal(siteOfDomain(domain), 'tiktok', domain);
    }
    for (const domain of ['.youtube.com', 'youtube.com', 'm.youtube.com', 'music.youtube.com']) {
      assert.equal(siteOfDomain(domain), 'youtube', domain);
    }
    // Someone else's, however much of the name they borrow -- TikTok's own
    // video and API hosts included, which the app's jar leaves out as well.
    for (const domain of [
      'tiktok.com.evil.test',
      '.nottiktok.com',
      '.tiktokv.com',
      'v16.tiktokcdn.com',
      '.notyoutube.com',
      'youtube.com.example',
      '.google.com',
      '',
      undefined,
    ]) {
      assert.equal(siteOfDomain(domain), null, String(domain));
    }
  });

  test('a jar is signed in only by a sign-in cookie that has a value', () => {
    // What TikTok and YouTube set for every visitor.
    assert.equal(signedIn('tiktok', [cookie('msToken'), cookie('ttwid'), cookie('tt_csrf_token')]), false);
    assert.equal(signedIn('youtube', [cookie('PREF'), cookie('VISITOR_INFO1_LIVE'), cookie('YSC')]), false);

    assert.equal(signedIn('tiktok', [cookie('msToken'), cookie('sessionid')]), true);
    assert.equal(signedIn('tiktok', [cookie('sid_tt')]), true);
    assert.equal(signedIn('youtube', [cookie('PREF'), cookie('__Secure-3PSID')]), true);

    // A signed-out browser can keep the name with nothing in it.
    assert.equal(signedIn('tiktok', [cookie('sessionid', '')]), false);
    // One site's sign-in says nothing about the other.
    assert.equal(signedIn('youtube', [cookie('sessionid')]), false);
    assert.equal(signedIn('tiktok', [cookie('SID')]), false);
    assert.equal(signedIn('tiktok', []), false);
  });

  test("TikTok's churn is no reason to start the helper; its sign-in is", () => {
    for (const name of ['msToken', 'ttwid', 'tt_chain_token', 'odin_tt']) {
      assert.equal(worthPushing('tiktok', name), false, name);
    }
    for (const name of ['sessionid', 'sessionid_ss', 'sid_tt', 'sid_guard', 'uid_tt']) {
      assert.equal(worthPushing('tiktok', name), true, name);
    }
    // Every YouTube change still is, as it was before there was a second site.
    assert.equal(worthPushing('youtube', 'PREF'), true);
    assert.equal(worthPushing('youtube', 'SID'), true);
  });

  test('a sign-in coming or going is always worth a push', () => {
    // A sign-in that is detected but never watched would reach the app only
    // with the daily alarm.
    for (const [site, { session }] of Object.entries(SITES)) {
      for (const name of session) assert.equal(worthPushing(site, name), true, `${site} ${name}`);
    }
  });

  test("only YouTube's session travels as a plain push", () => {
    // Exactly what every extension before 1.0.5 sent: an older host reads it,
    // and one that sees a `site` on it refuses it.
    assert.deepEqual(pushHead('youtube'), { type: 'push' });
    assert.deepEqual(pushHead('tiktok'), { type: 'pushSite', site: 'tiktok' });
    // Any other site's cookies sent as `push` would be stored as YouTube's by
    // a host from before the second site.
    for (const site of Object.keys(SITES).filter((site) => site !== 'youtube')) {
      assert.equal(pushHead(site).type, 'pushSite', site);
      assert.equal(pushHead(site).site, site);
    }
  });

  test("TikTok's switch is offered on TikTok's own pages only", () => {
    for (const url of [
      'https://www.tiktok.com/@someone/video/7670756126907960589',
      'https://vm.tiktok.com/ZMabc/',
      'https://vt.tiktok.com/ZSabc/',
      'https://tiktok.com/',
      'http://m.tiktok.com/v/1.html',
    ]) {
      assert.equal(onTikTok(url), true, url);
    }
    for (const url of [
      'javascript:alert(1)',
      'chrome://extensions/',
      'https://tiktok.com.evil.test/@someone/video/1',
      'https://www.nottiktok.com/',
      'https://example.com/?next=https://www.tiktok.com/',
      'https://www.youtube.com/watch?v=1',
      'not an address',
      '',
      undefined,
    ]) {
      assert.equal(onTikTok(url), false, String(url));
    }
  });

  test("YouTube's switch is offered on YouTube's own pages only, short links included", () => {
    for (const url of [
      'https://www.youtube.com/watch?v=1',
      'https://music.youtube.com/',
      'https://m.youtube.com/shorts/1',
      'https://youtu.be/abc',
    ]) {
      assert.equal(onYouTube(url), true, url);
    }
    for (const url of [
      'https://www.tiktok.com/@a/video/1',
      'https://youtube.com.evil.test/watch?v=1',
      'https://www.youtube-nocookie.com/embed/1',
      'https://example.com/?next=https://www.youtube.com/',
      'chrome://extensions/',
      '',
      undefined,
    ]) {
      assert.equal(onYouTube(url), false, String(url));
    }
  });

  test('an address is lent the session of the site it is on, not the page it came from', () => {
    assert.equal(siteOfUrl('https://www.tiktok.com/@someone/video/1'), 'tiktok');
    assert.equal(siteOfUrl('https://music.youtube.com/watch?v=1'), 'youtube');
    assert.equal(siteOfUrl('https://blog.example/post-with-a-tiktok-embed'), null);
    assert.equal(siteOfUrl('https://v16-webapp.tiktokcdn.com/video.mp4'), null);
  });

  test('the sites are the ones the app knows by name', () => {
    // A `pushSite` naming a site the host does not know fails to read at all.
    // The other sites' slot is the last of them, and not one of SITES.
    const source = read('src-tauri', 'src', 'bridge', 'protocol.rs');
    const block = /pub enum Site \{([^}]*)\}/.exec(source);
    assert.ok(block, 'enum Site not found in protocol.rs');
    const names = block[1]
      .split(',')
      .map((variant) => variant.trim())
      .filter(Boolean)
      // serde's camelCase: the first letter lowered.
      .map((variant) => variant[0].toLowerCase() + variant.slice(1));
    assert.deepEqual([...Object.keys(SITES), OTHER], names);
    assert.deepEqual(pushHead(OTHER), { type: 'pushSite', site: 'other' });
  });

  test('a registrable domain is the last two labels, or three under a country', () => {
    for (const [host, domain] of [
      ['www.bbc.co.uk', 'bbc.co.uk'],
      ['x.com', 'x.com'],
      ['m.facebook.com', 'facebook.com'],
      ['www.trendyol.com.tr', 'trendyol.com.tr'],
      ['WWW.Instagram.COM', 'instagram.com'],
      ['.instagram.com', 'instagram.com'],
      ['old.reddit.com.', 'reddit.com'],
      ['a.b.c.example.ac.jp', 'example.ac.jp'],
      ['www.example.de', 'example.de'],
      ['bit.ly', 'bit.ly'],
      ['xn--80ak6aa92e.com', 'xn--80ak6aa92e.com'],
    ]) {
      assert.equal(registrableDomain(host), domain, host);
    }
    for (const host of ['co.uk', 'com.tr', 'localhost', '192.168.1.10', '', 'exa_mple.com', '-bad.com', undefined]) {
      assert.equal(registrableDomain(host), null, String(host));
    }
  });

  test("the other sites' switch is for any web page but YouTube's and TikTok's", () => {
    for (const [url, domain] of [
      ['https://www.instagram.com/p/DQ3zR6-DPGm/', 'instagram.com'],
      ['https://x.com/someone/status/1', 'x.com'],
      ['https://m.facebook.com/watch/?v=1', 'facebook.com'],
      ['https://vimeo.com/76979871', 'vimeo.com'],
      ['https://old.reddit.com/r/videos/', 'reddit.com'],
      ['https://www.bbc.co.uk/iplayer', 'bbc.co.uk'],
      ['http://www.trendyol.com.tr/', 'trendyol.com.tr'],
    ]) {
      assert.equal(otherSiteOf(url), domain, url);
    }
    for (const url of [
      'https://www.youtube.com/watch?v=1',
      'https://music.youtube.com/watch?v=1',
      'https://youtu.be/1',
      'https://www.youtube-nocookie.com/embed/1',
      'https://accounts.google.com/',
      'https://www.google.com.tr/search?q=a',
      'https://www.tiktok.com/@a/video/1',
      'https://v16.tiktokcdn.com/a.mp4',
      'http://localhost:1420/',
      'http://127.0.0.1/video.mp4',
      'http://[::1]/video.mp4',
      'chrome://extensions/',
      'file:///C:/video.mp4',
      'not an address',
      '',
      undefined,
    ]) {
      assert.equal(otherSiteOf(url), null, String(url));
    }
  });

  test('every name the app keeps for its own switches is kept here too', () => {
    // The host refuses an other-site push for any of these; one the extension
    // still offered would be a push refused whole on every İndir.
    const source = read('src-tauri', 'src', 'bridge', 'protocol.rs');
    const block = /const OWN_SWITCH_NAMES: \[&str; \d+\] = \[([\s\S]*?)\];/.exec(source);
    assert.ok(block, 'OWN_SWITCH_NAMES not found in protocol.rs');
    const names = [...block[1].matchAll(/"([^"]+)"/g)].map((match) => match[1]);
    assert.ok(names.length > 0);
    for (const name of names) {
      assert.equal(otherSiteOf(`https://www.${name}.com/`), null, name);
      assert.equal(otherSiteOf(`https://www.${name}.co.uk/`), null, name);
    }
  });

  test("every sign-in cookie named here is one the app's log masks", () => {
    const source = read('src-tauri', 'src', 'bridge', 'mod.rs');
    const block = /pub const SENSITIVE_COOKIE_NAMES: &\[&str\] = &\[([\s\S]*?)\];/.exec(source);
    assert.ok(block, 'SENSITIVE_COOKIE_NAMES not found in bridge/mod.rs');
    const masked = new Set([...block[1].matchAll(/"([^"]+)"/g)].map((match) => match[1]));
    for (const [site, { session, watch }] of Object.entries(SITES)) {
      for (const name of [...session, ...(watch ?? [])]) {
        assert.ok(masked.has(name), `${site}: ${name} is not masked in the app's log`);
      }
    }
  });
});

describe('messages', () => {
  const messages = (locale) => JSON.parse(read('extension', '_locales', locale, 'messages.json'));
  const en = messages('en');
  const tr = messages('tr');

  test('both languages say every message', () => {
    assert.deepEqual(Object.keys(tr).sort(), Object.keys(en).sort());
    for (const [key, entry] of [...Object.entries(en), ...Object.entries(tr)]) {
      assert.ok(typeof entry.message === 'string' && entry.message.trim() !== '', key);
    }
  });

  test('every message a page names exists', () => {
    for (const page of ['popup.html', 'welcome.html']) {
      const html = read('extension', page);
      for (const [, key] of html.matchAll(/data-i18n(?:-title)?="([^"]+)"/g)) {
        assert.ok(Object.hasOwn(en, key), `${page}: ${key}`);
      }
    }
  });
});

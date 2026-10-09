/**
 * What Settings → Connection says. The extension's row sits above the
 * browser sign-ins' own state, so the two must never disagree: a row asking
 * the user to install the extension above one saying a browser is signed in
 * through it reads as a broken page. And that state is taken across every
 * site the browser lends a sign-in for, so a working TikTok session is never
 * reported as no session at all.
 */

import { describe, expect, test, vi } from 'vitest';

import type { BridgeStatus } from '@/types';

// The card imports the platform check, which reads the OS plugin on load.
vi.mock('@tauri-apps/plugin-os', () => ({ platform: () => 'windows' }));

const { extensionOffer, lastRefreshed, otherSite, phaseOf } = await import('./BrowserLinkCard');

function status(patch: Partial<BridgeStatus> = {}): BridgeStatus {
  return {
    supported: true,
    storeListed: true,
    enabled: true,
    registered: true,
    connected: false,
    browser: null,
    profileLabel: null,
    accountHint: null,
    extensionVersion: null,
    lastPushAt: null,
    session: 'none',
    tiktokSession: 'none',
    tiktokLastPushAt: null,
    otherSession: 'none',
    otherDomain: null,
    hostPath: null,
    appVersion: '0.0.0',
    extensionId: 'oikcjjcihkfmgmmmjagilnfgnfilghic',
    ...patch,
  };
}

describe('extensionOffer', () => {
  test('offers nothing before the first read answers', () => {
    expect(extensionOffer(null)).toBeNull();
  });

  test('points to the store while no browser is bound', () => {
    expect(extensionOffer(status())).toBe('store');
  });

  test('says the listing is pending when there is none yet', () => {
    expect(extensionOffer(status({ storeListed: false }))).toBe('storePending');
  });

  test('stops asking once a bound browser proves the extension is installed', () => {
    const bound = status({ connected: true, browser: 'Chrome', extensionVersion: '1.0.4' });
    expect(extensionOffer(bound)).toBeNull();
    expect(extensionOffer({ ...bound, storeListed: false })).toBeNull();
  });

  test('keeps the binding as proof with the browser sign-ins switched off', () => {
    // Turning the switch off drops the sessions but not the binding, and the
    // extension is no less installed for it.
    expect(extensionOffer(status({ connected: true, enabled: false }))).toBeNull();
  });
});

describe('phaseOf', () => {
  const bound = (patch: Partial<BridgeStatus>) => status({ connected: true, ...patch });

  test('waits for a browser before saying anything about sign-ins', () => {
    expect(phaseOf(status({ session: 'fresh', tiktokSession: 'fresh' }))).toBe('waiting');
  });

  test('is connected while either site’s sign-in is fresh', () => {
    expect(phaseOf(bound({ session: 'fresh' }))).toBe('connected');
    expect(phaseOf(bound({ tiktokSession: 'fresh' }))).toBe('connected');
    expect(phaseOf(bound({ session: 'stale', tiktokSession: 'fresh' }))).toBe('connected');
  });

  test('has gone quiet when a sign-in is held but none is fresh', () => {
    expect(phaseOf(bound({ session: 'stale' }))).toBe('quiet');
    expect(phaseOf(bound({ tiktokSession: 'stale' }))).toBe('quiet');
  });

  test('is signed out when neither site has a sign-in stored', () => {
    expect(phaseOf(bound({}))).toBe('signedOut');
  });

  test('takes no account of the other-site sign-in, which lasts an hour', () => {
    // Lent for one press of İndir; reading "connected" for the hour after a
    // download and "signed out" after it would say nothing about the link.
    expect(phaseOf(bound({ otherSession: 'fresh', otherDomain: 'instagram.com' }))).toBe(
      'signedOut',
    );
  });
});

describe('otherSite', () => {
  test('names the site while its sign-in is held', () => {
    expect(otherSite(status({ otherSession: 'fresh', otherDomain: 'instagram.com' }))).toBe(
      'instagram.com',
    );
  });

  test('names nothing when none is held, or there is no site to name', () => {
    expect(otherSite(status())).toBeNull();
    expect(otherSite(status({ otherSession: 'fresh' }))).toBeNull();
    expect(otherSite(status({ otherDomain: 'instagram.com' }))).toBeNull();
  });
});

describe('lastRefreshed', () => {
  test('is the newer of the two pushes', () => {
    expect(lastRefreshed(status({ lastPushAt: 100, tiktokLastPushAt: 200 }))).toBe(200);
    expect(lastRefreshed(status({ lastPushAt: 300, tiktokLastPushAt: 200 }))).toBe(300);
    expect(lastRefreshed(status({ tiktokLastPushAt: 200 }))).toBe(200);
    expect(lastRefreshed(status())).toBeNull();
  });
});

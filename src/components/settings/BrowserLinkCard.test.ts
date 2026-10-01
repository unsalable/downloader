/**
 * What the extension's row in Settings → Connection offers. The row sits
 * above the YouTube sign-in's own state, so the two must never disagree: a
 * row asking the user to install the extension above one saying a browser is
 * signed in through it reads as a broken page.
 */

import { describe, expect, test, vi } from 'vitest';

import type { BridgeStatus } from '@/types';

// The card imports the platform check, which reads the OS plugin on load.
vi.mock('@tauri-apps/plugin-os', () => ({ platform: () => 'windows' }));

const { extensionOffer } = await import('./BrowserLinkCard');

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

  test('keeps the binding as proof with the YouTube sign-in switched off', () => {
    // Turning the switch off drops the session but not the binding, and the
    // extension is no less installed for it.
    expect(extensionOffer(status({ connected: true, enabled: false }))).toBeNull();
  });
});

/**
 * What a failure is called and what is said about it. A phone has sentences
 * of its own where the desktop's advice -- connect a browser, turn on its
 * session -- cannot be followed there, and it must never be handed the
 * desktop's; the desktop must never be handed the phone's either.
 */

import { beforeEach, describe, expect, test, vi } from 'vitest';

import type { AppErrorInfo } from '@/types';

import { en, type TranslationKey } from './en';
import { errorCopy, errorMessage, setLanguage } from './index';
import { tr } from './tr';

// Switching the language names it on the page, and there is no page here.
vi.stubGlobal('document', { documentElement: { lang: 'en' } });

function failure(code: string): AppErrorInfo {
  return {
    code,
    title: 'What the backend called it',
    message: 'What the backend said about it',
    technical: null,
    retryable: false,
  };
}

beforeEach(() => setLanguage('en'));

describe('errorCopy', () => {
  test('a phone is told what a phone can do', () => {
    expect(errorCopy(failure('tiktokSignIn'), true)).toEqual({
      title: en['error.tiktokSignIn.title'],
      message: en['error.tiktokSignIn.messageMobile'],
    });
    expect(errorMessage(failure('membershipRequired'), true)).toBe(
      en['error.membershipRequired.messageMobile'],
    );
  });

  test('the desktop never takes the phone’s sentence', () => {
    expect(errorCopy(failure('tiktokSignIn')).message).toBe(en['error.tiktokSignIn.message']);
    expect(errorCopy(failure('tiktokSignIn'), false).message).toBe(
      en['error.tiktokSignIn.message'],
    );
  });

  test('a code with no sentence for the phone says the shared one there', () => {
    expect(errorCopy(failure('tiktokNotForAccount'), true)).toEqual({
      title: en['error.tiktokNotForAccount.title'],
      message: en['error.tiktokNotForAccount.message'],
    });
  });

  test('a code no dictionary knows keeps the backend’s own words', () => {
    expect(errorCopy(failure('somethingNew'), true)).toEqual({
      title: 'What the backend called it',
      message: 'What the backend said about it',
    });
  });

  test('speaks the chosen language', () => {
    setLanguage('tr');
    expect(errorCopy(failure('tiktokSignIn'), true)).toEqual({
      title: tr['error.tiktokSignIn.title'],
      message: tr['error.tiktokSignIn.messageMobile'],
    });
  });

  test('every sentence for the phone stands beside one for the desktop', () => {
    const phoneKeys = Object.keys(en).filter((key) => /^error\..+\.messageMobile$/.test(key));
    expect(phoneKeys.length).toBeGreaterThan(0);
    for (const key of phoneKeys) {
      const shared = key.replace(/Mobile$/, '') as TranslationKey;
      expect(en[shared], shared).toBeTruthy();
      expect(tr[key as TranslationKey], key).toBeTruthy();
    }
  });
});

import { describe, expect, it } from 'vitest';

import { en } from '@/i18n/en';
import { tr } from '@/i18n/tr';
import type { AppErrorInfo } from '@/types';

import { PROBLEM_TEXT, problemFrom, type FileAction } from './fileProblem';

const ACTIONS: FileAction[] = ['open', 'reveal', 'share'];

const failure = (code: string): AppErrorInfo => ({
  code,
  title: '',
  message: '',
  technical: null,
  retryable: false,
});

describe('problemFrom', () => {
  it('a missing file is said to be missing', () => {
    for (const action of ACTIONS) {
      expect(problemFrom(failure('fileMissing'), action)).toBe('missing');
    }
  });

  it('any other failure names the action', () => {
    for (const code of ['unknown', 'permission']) {
      for (const action of ACTIONS) {
        expect(problemFrom(failure(code), action)).toBe(action);
      }
    }
  });

  // What every desktop row used to get: the opener's refusal, a plain string,
  // read as the file having gone.
  it("the plugin's string refusal is not mistaken for a missing file", () => {
    expect(problemFrom('Not allowed to open path C:\\x.mp4', 'open')).toBe('open');
    expect(problemFrom(undefined, 'share')).toBe('share');
    expect(problemFrom(null, 'reveal')).toBe('reveal');
    expect(problemFrom({ message: 'fileMissing' }, 'open')).toBe('open');
  });
});

describe('PROBLEM_TEXT', () => {
  it('every problem has words in both languages', () => {
    for (const key of Object.values(PROBLEM_TEXT)) {
      expect(en[key]).toBeTruthy();
      expect(tr[key]).toBeTruthy();
    }
  });
});

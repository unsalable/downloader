import { describe, expect, it } from 'vitest';

import { en } from '@/i18n/en';
import { tr } from '@/i18n/tr';

import { CLEAR_RANGES, DEFAULT_CLEAR_RANGE, RANGE_LABEL, clearSummaryKey } from './clearRange';

describe('clearSummaryKey', () => {
  it('has a sentence for none, for one and for several', () => {
    for (const range of CLEAR_RANGES) {
      expect(clearSummaryKey(range, 0)).toBe(`history.clearCount.${range}.none`);
      expect(clearSummaryKey(range, 1)).toBe(`history.clearCount.${range}.one`);
      expect(clearSummaryKey(range, 12)).toBe(`history.clearCount.${range}.many`);
    }
  });

  // The count is put into the sentence, so a sentence with a number in it that
  // has nowhere to put it would say less than the dialog knows.
  it('is written in both languages, with a place for the number when there is one', () => {
    for (const range of CLEAR_RANGES) {
      for (const n of [0, 1, 2]) {
        const key = clearSummaryKey(range, n);
        for (const dictionary of [en, tr]) {
          expect(dictionary[key]).toBeTruthy();
          if (n > 0) expect(dictionary[key]).toContain('{n}');
        }
      }
    }
  });
});

describe('the ranges', () => {
  it('run from the narrowest, which is where the dialog opens', () => {
    expect(CLEAR_RANGES).toEqual(['day', 'week', 'all']);
    expect(DEFAULT_CLEAR_RANGE).toBe('day');
  });

  it('are named in both languages, as is everything around them', () => {
    for (const range of CLEAR_RANGES) {
      expect(en[RANGE_LABEL[range]]).toBeTruthy();
      expect(tr[RANGE_LABEL[range]]).toBeTruthy();
    }
    for (const key of ['history.clear', 'history.clearRange', 'history.clearFailed'] as const) {
      expect(en[key]).toBeTruthy();
      expect(tr[key]).toBeTruthy();
    }
  });
});

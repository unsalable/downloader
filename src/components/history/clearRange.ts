import type { TranslationKey } from '@/i18n';
import type { HistoryRange } from '@/types';

/*
 * The parts of clearing the history that are only words and order: kept apart
 * from the dialog so they can be tested without the window it runs in.
 */

/** The choices, in the order the rows show them: narrowest first. */
export const CLEAR_RANGES = ['day', 'week', 'all'] as const satisfies readonly HistoryRange[];

/**
 * Where every opening starts: the least that can be taken. A Clear pressed
 * without reading takes a day, not the lot. Not remembered between openings.
 */
export const DEFAULT_CLEAR_RANGE: HistoryRange = 'day';

export const RANGE_LABEL: Record<HistoryRange, TranslationKey> = {
  day: 'history.range.day',
  week: 'history.range.week',
  all: 'history.range.all',
};

/**
 * The sentence under the choices, in its words for none, one or several. Built
 * as a template the compiler checks against the dictionary, so a range without
 * its three sentences does not build.
 */
export function clearSummaryKey(range: HistoryRange, count: number): TranslationKey {
  const amount = count === 0 ? 'none' : count === 1 ? 'one' : 'many';
  return `history.clearCount.${range}.${amount}`;
}

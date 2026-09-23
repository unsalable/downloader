/**
 * The custom bitrate's arithmetic: what a typed number means, what the prefill
 * guesses, and how big the result is said to be.
 */

import { describe, expect, test } from 'vitest';

import {
  FALLBACK_KBPS,
  estimatedBytes,
  formatMbps,
  parseMbps,
  roundMbps,
  sourceVideoKbps,
} from '@/lib/editor/bitrate';

describe('reading what was typed', () => {
  test('whole numbers, decimals, and either decimal mark', () => {
    expect(parseMbps('8')).toBe(8000);
    expect(parseMbps('2.5')).toBe(2500);
    expect(parseMbps('2,5')).toBe(2500);
    expect(parseMbps(' 12 ')).toBe(12000);
    expect(parseMbps('.5')).toBe(500);
  });

  test('a trailing mark is the middle of typing, not a mistake', () => {
    expect(parseMbps('2.')).toBe(2000);
  });

  test('the ends of the range are in it, and past them is not', () => {
    expect(parseMbps('0.1')).toBe(100);
    expect(parseMbps('300')).toBe(300000);
    expect(parseMbps('0.05')).toBeNull();
    expect(parseMbps('0')).toBeNull();
    expect(parseMbps('300.1')).toBeNull();
  });

  test('anything that is not a number is refused', () => {
    expect(parseMbps('')).toBeNull();
    expect(parseMbps('abc')).toBeNull();
    expect(parseMbps('-3')).toBeNull();
    expect(parseMbps('1e2')).toBeNull();
    expect(parseMbps('2.5.1')).toBeNull();
  });
});

describe('writing it back', () => {
  test('in the language the user reads', () => {
    expect(formatMbps(2500, 'en')).toBe('2.5');
    expect(formatMbps(2500, 'tr')).toBe('2,5');
    expect(formatMbps(8000, 'tr')).toBe('8');
    expect(formatMbps(120000, 'en')).toBe('120');
  });
});

describe('guessing the source', () => {
  test('rounds the way a person would say it', () => {
    expect(roundMbps(164.6)).toBe(165);
    expect(roundMbps(35.2)).toBe(35);
    expect(roundMbps(8.4)).toBe(8.5);
    expect(roundMbps(0.83)).toBe(0.8);
  });

  test('takes the audio off the file rate', () => {
    // 21 MB over 20 s is 8.4 Mbps; less 192 kbps of audio is 8.2, said as 8.
    const kbps = sourceVideoKbps({
      sizeBytes: 21_000_000,
      durationSec: 20,
      hasAudio: true,
      audioBitrateKbps: 192,
    });
    expect(kbps).toBe(8000);
  });

  test('a file with nothing to go on falls back', () => {
    expect(
      sourceVideoKbps({ sizeBytes: 0, durationSec: 20, hasAudio: false, audioBitrateKbps: null }),
    ).toBe(FALLBACK_KBPS);
    expect(
      sourceVideoKbps({ sizeBytes: 1000, durationSec: null, hasAudio: false, audioBitrateKbps: null }),
    ).toBe(FALLBACK_KBPS);
    // Audio claiming more than the whole file carries.
    expect(
      sourceVideoKbps({ sizeBytes: 10_000, durationSec: 10, hasAudio: true, audioBitrateKbps: 320 }),
    ).toBe(FALLBACK_KBPS);
  });

  test('stays inside what the field accepts', () => {
    expect(
      sourceVideoKbps({ sizeBytes: 50_000_000_000, durationSec: 10, hasAudio: false, audioBitrateKbps: null }),
    ).toBe(300000);
  });
});

describe('the size it comes to', () => {
  test('rate times length, in bytes', () => {
    // 8 Mbps of video and 192 kbps of audio for ten seconds.
    expect(estimatedBytes(8192, 10)).toBe(10_240_000);
  });
});

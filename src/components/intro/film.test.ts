/**
 * The intro film's timeline and words, against the promises the host and the
 * scenes make each other: every scene plays inside the film, one hands over to
 * the next with no empty frame between them, the button arrives before the
 * film stops, and every word the film says exists in both languages.
 *
 * The film's look is checked by eye, with scripts/intro-video/render.mjs;
 * these are the parts a later retiming could break without anyone seeing it.
 */

import { describe, expect, test, vi } from 'vitest';

import { en } from '@/i18n/en';
import { tr } from '@/i18n/tr';

// The scenes import the platform list, which reads the OS plugin on load.
vi.mock('@tauri-apps/plugin-os', () => ({ platform: () => 'android' }));

const { INTRO_CTA_FROM, INTRO_FRAMES, INTRO_SCENES } = await import('./IntroVideo');
const { introStrings } = await import('./strings');

describe('the film', () => {
  test('every scene plays inside it', () => {
    for (const scene of INTRO_SCENES) {
      expect(scene.from, scene.id).toBeGreaterThanOrEqual(0);
      expect(scene.from + scene.duration, scene.id).toBeLessThanOrEqual(INTRO_FRAMES);
    }
  });

  test('each scene is mounted by the time the one before it leaves', () => {
    const ordered = [...INTRO_SCENES].sort((a, b) => a.from - b.from);
    expect(ordered[0]!.from).toBe(0);
    for (let index = 1; index < ordered.length; index += 1) {
      const before = ordered[index - 1]!;
      expect(ordered[index]!.from, ordered[index]!.id).toBeLessThanOrEqual(before.from + before.duration);
    }
    const last = ordered[ordered.length - 1]!;
    expect(last.from + last.duration).toBe(INTRO_FRAMES);
  });

  test('the button arrives while the last scene is still up', () => {
    const last = [...INTRO_SCENES].sort((a, b) => a.from - b.from).at(-1)!;
    expect(INTRO_CTA_FROM).toBeGreaterThan(last.from);
    expect(INTRO_CTA_FROM).toBeLessThan(INTRO_FRAMES);
  });

  test('says every word in both languages', () => {
    for (const dictionary of [en, tr]) {
      const strings = introStrings((key) => dictionary[key]);
      for (const [name, text] of Object.entries(strings)) {
        expect(text.trim(), name).not.toBe('');
      }
    }
  });
});

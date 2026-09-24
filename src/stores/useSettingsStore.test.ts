/**
 * The first launch after install speaks the system's language, and every
 * later one speaks what is stored. "First" is the backend's answer -- nothing
 * has been saved yet -- rather than a guess from the values themselves, so a
 * language the user picked stays picked even while onboarding is unfinished.
 */

import { beforeEach, describe, expect, test, vi } from 'vitest';

import type { LanguageCode, Settings } from '@/types';

const backend = vi.hoisted(() => ({
  stored: null as unknown as Settings,
  /** `null` is what the browser preview answers to a command it lacks. */
  firstLaunch: false as boolean | null,
  saveFails: false,
  saved: [] as Settings[],
}));

vi.mock('@tauri-apps/api/window', () => ({
  getCurrentWindow: () => ({ setTheme: async () => {} }),
}));
vi.mock('motion/react', () => ({ MotionGlobalConfig: {} }));
vi.mock('@/lib/platform', () => ({ IS_MOBILE: false }));
vi.mock('@/services/ipc', () => ({
  getSettings: async () => backend.stored,
  isFirstLaunch: async () => backend.firstLaunch,
  saveSettings: async (settings: Settings) => {
    if (backend.saveFails) throw new Error('the disk is full');
    backend.saved.push(settings);
    backend.stored = settings;
    return settings;
  },
  // As the backend does: every default back in the language given, onboarding
  // kept, in one save.
  resetSettings: async (language?: LanguageCode) => {
    backend.stored = {
      ...DEFAULTS,
      language: language ?? DEFAULTS.language,
      onboardingComplete: backend.stored.onboardingComplete,
    };
    backend.saved.push(backend.stored);
    return backend.stored;
  },
  platformSetSystemBars: async () => {},
}));

vi.stubGlobal('document', {
  documentElement: { lang: 'en', dataset: {}, style: {}, classList: { toggle: () => {} } },
});

const { detectSystemLanguage, getLanguage, setLanguage } = await import('@/i18n');
const { useSettingsStore } = await import('@/stores/useSettingsStore');

/** The system's preferred languages, most preferred first. */
function systemSpeaks(...languages: string[]) {
  vi.stubGlobal('navigator', { language: languages[0], languages });
}

const DEFAULTS = {
  language: 'en',
  theme: 'dark',
  reduceMotion: false,
  lowResourceMode: false,
  onboardingComplete: false,
} as Settings;

const load = () => useSettingsStore.getState().load();
const loaded = () => useSettingsStore.getState().settings;

beforeEach(() => {
  backend.stored = { ...DEFAULTS };
  backend.firstLaunch = false;
  backend.saveFails = false;
  backend.saved = [];
  setLanguage('en');
  useSettingsStore.setState({ settings: null, loading: true });
});

describe('the first launch', () => {
  test('takes the system language, and saves it so it is a choice from then on', async () => {
    systemSpeaks('tr-TR', 'en-US');
    backend.firstLaunch = true;

    await load();

    expect(loaded()?.language).toBe('tr');
    expect(getLanguage()).toBe('tr');
    expect(backend.saved.map((settings) => settings.language)).toEqual(['tr']);
  });

  test('still speaks the system language when the save fails', async () => {
    systemSpeaks('tr-TR');
    backend.firstLaunch = true;
    backend.saveFails = true;

    await load();

    expect(loaded()?.language).toBe('tr');
    expect(useSettingsStore.getState().loading).toBe(false);
  });
});

describe('a later launch', () => {
  test('keeps a language picked before onboarding was finished', async () => {
    systemSpeaks('tr-TR');
    backend.stored = { ...DEFAULTS, language: 'en', onboardingComplete: false };

    await load();

    expect(loaded()?.language).toBe('en');
    expect(backend.saved).toEqual([]);
  });

  test('is assumed when the backend cannot say', async () => {
    systemSpeaks('tr-TR');
    backend.firstLaunch = null;

    await load();

    expect(loaded()?.language).toBe('en');
    expect(backend.saved).toEqual([]);
  });
});

describe('a reset', () => {
  test('goes back to the system language, as a first launch does', async () => {
    systemSpeaks('tr-TR');
    backend.stored = { ...DEFAULTS, language: 'en', onboardingComplete: true };
    await load();

    await useSettingsStore.getState().reset();

    expect(loaded()?.language).toBe('tr');
    expect(loaded()?.onboardingComplete).toBe(true);
    // One save, already in Turkish: never English for a moment in between.
    expect(backend.saved.map((settings) => settings.language)).toEqual(['tr']);
  });
});

describe('the system language', () => {
  test('is the first preferred language the app speaks', () => {
    systemSpeaks('de-DE', 'tr-TR');
    expect(detectSystemLanguage()).toBe('tr');
    systemSpeaks('de-DE', 'en-GB', 'tr');
    expect(detectSystemLanguage()).toBe('en');
    systemSpeaks('TR');
    expect(detectSystemLanguage()).toBe('tr');
  });

  test('falls back to English', () => {
    systemSpeaks('fr-FR', 'de-DE');
    expect(detectSystemLanguage()).toBe('en');
  });
});

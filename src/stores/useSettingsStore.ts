import { getCurrentWindow, Theme } from '@tauri-apps/api/window';
import { MotionGlobalConfig } from 'motion/react';
import { create } from 'zustand';

import { setLanguage } from '@/i18n';
import { IS_MOBILE } from '@/lib/platform';
import * as ipc from '@/services/ipc';
import type { Settings, ThemePreference } from '@/types';

interface SettingsState {
  settings: Settings | null;
  loading: boolean;
  saving: boolean;
  load: () => Promise<void>;
  /** Optimistic: the UI updates immediately, then reconciles with what Rust
   *  returns after sanitising (clamped numbers, normalised empty strings). */
  update: (patch: Partial<Settings>) => Promise<void>;
  reset: () => Promise<void>;
  applyExternal: (settings: Settings) => void;
}

function applySideEffects(settings: Settings) {
  setLanguage(settings.language);
  applyTheme(settings.theme);
  applyMotion(settings);
}

/**
 * "Reduce motion" has two audiences. The stylesheet reads the data attributes
 * for CSS transitions and keyframes; Motion runs its animations in JavaScript
 * and never sees CSS, so it is told through its global config. That flag is
 * read as each animation starts, so the change applies from the next animation
 * on without remounting anything.
 *
 * Low resource mode promises fewer visual effects, and movement is most of
 * them, so it implies reduced motion without flipping the user's own toggle.
 */
function applyMotion(settings: Settings) {
  const reduce = settings.reduceMotion || settings.lowResourceMode;
  const root = document.documentElement;
  root.dataset.reduceMotion = reduce ? 'true' : 'false';
  root.dataset.lowResource = settings.lowResourceMode ? 'true' : 'false';
  MotionGlobalConfig.skipAnimations = reduce;
}

let systemThemeQuery: MediaQueryList | null = null;
let systemThemeListener: ((event: MediaQueryListEvent) => void) | null = null;

/**
 * Theme is a class on <html>. "System" installs a listener so the app follows
 * the OS while that option is selected, and removes it as soon as it is not --
 * an always-on listener would fight an explicit choice.
 */
export function applyTheme(preference: ThemePreference) {
  const root = document.documentElement;

  if (systemThemeQuery && systemThemeListener) {
    systemThemeQuery.removeEventListener('change', systemThemeListener);
    systemThemeListener = null;
  }

  const setClass = (dark: boolean) => {
    root.classList.toggle('dark', dark);
    root.style.colorScheme = dark ? 'dark' : 'light';
    // A phone's status and navigation bars are drawn over the page's own
    // background, so their icons have to follow this theme, not the OS one.
    if (IS_MOBILE) void ipc.platformSetSystemBars(dark).catch(() => {});
  };

  // The native title bar is painted by Windows, not by this stylesheet, so it
  // has to be told separately. `null` hands the decision back to the OS, which
  // is exactly what "System" means.
  const nativeTheme: Theme | null =
    preference === 'system' ? null : preference === 'dark' ? 'dark' : 'light';
  void getCurrentWindow()
    .setTheme(nativeTheme)
    .catch(() => {
      // Not fatal: the window keeps whatever theme it had.
    });

  if (preference === 'system') {
    systemThemeQuery ??= window.matchMedia('(prefers-color-scheme: dark)');
    setClass(systemThemeQuery.matches);
    systemThemeListener = (event) => setClass(event.matches);
    systemThemeQuery.addEventListener('change', systemThemeListener);
    return;
  }

  setClass(preference === 'dark');
}

export const useSettingsStore = create<SettingsState>((set, get) => ({
  settings: null,
  loading: true,
  saving: false,

  load: async () => {
    set({ loading: true });
    try {
      const settings = await ipc.getSettings();
      applySideEffects(settings);
      set({ settings, loading: false });
    } catch {
      set({ loading: false });
    }
  },

  update: async (patch) => {
    const current = get().settings;
    if (!current) return;

    const optimistic = { ...current, ...patch };
    applySideEffects(optimistic);
    set({ settings: optimistic, saving: true });

    try {
      const saved = await ipc.saveSettings(optimistic);
      applySideEffects(saved);
      set({ settings: saved, saving: false });
    } catch {
      // Put the previous values back rather than leaving the UI showing a
      // setting that was never persisted.
      applySideEffects(current);
      set({ settings: current, saving: false });
    }
  },

  reset: async () => {
    set({ saving: true });
    const settings = await ipc.resetSettings();
    applySideEffects(settings);
    set({ settings, saving: false });
  },

  applyExternal: (settings) => {
    applySideEffects(settings);
    set({ settings });
  },
}));

/** Settings are loaded before first paint, so components can rely on them. */
export function useSettings(): Settings | null {
  return useSettingsStore((state) => state.settings);
}

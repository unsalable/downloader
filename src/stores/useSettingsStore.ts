import { getCurrentWindow, Theme } from '@tauri-apps/api/window';
import { create } from 'zustand';

import { setLanguage } from '@/i18n';
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
  document.documentElement.dataset.reduceMotion = settings.reduceMotion ? 'true' : 'false';
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

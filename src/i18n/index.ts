import { useSyncExternalStore } from 'react';

import type { LanguageCode } from '@/types';
import { en, type TranslationKey } from './en';
import { tr } from './tr';

const dictionaries: Record<LanguageCode, Record<TranslationKey, string>> = {
  en,
  tr,
};

export const LANGUAGES: { code: LanguageCode; label: string }[] = [
  { code: 'en', label: 'English' },
  { code: 'tr', label: 'Türkçe' },
];

/**
 * i18n keeps its own tiny store rather than living in the settings store.
 * Settings pushes into it on load and on change, which keeps the dependency
 * one-directional (settings -> i18n) and lets `t` be called from modules that
 * settings itself imports.
 */
let current: LanguageCode = 'en';
const listeners = new Set<() => void>();

function emit() {
  for (const listener of listeners) listener();
}

export function setLanguage(next: LanguageCode) {
  if (next === current || !dictionaries[next]) return;
  current = next;
  document.documentElement.lang = next;
  emit();
}

export function getLanguage(): LanguageCode {
  return current;
}

function subscribe(listener: () => void) {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

export type TranslateValues = Record<string, string | number>;

/**
 * Look up `key` in the active dictionary and substitute {placeholders}.
 * Falls back to English, then to the key itself, so a missing string is
 * visible in development without throwing at runtime.
 */
export function translate(key: TranslationKey, values?: TranslateValues): string {
  const template = dictionaries[current][key] ?? en[key] ?? key;
  if (!values) return template;
  return template.replace(/\{(\w+)\}/g, (match, name: string) =>
    name in values ? String(values[name]) : match,
  );
}

/**
 * Subscribing to the language store is what makes a language switch repaint
 * instantly, with no reload and no prop drilling.
 */
export function useTranslation() {
  const language = useSyncExternalStore(subscribe, getLanguage, getLanguage);
  return { t: translate, language };
}

export function detectSystemLanguage(): LanguageCode {
  const nav = navigator.language?.toLowerCase() ?? 'en';
  if (nav.startsWith('tr')) return 'tr';
  return 'en';
}

export type { TranslationKey };

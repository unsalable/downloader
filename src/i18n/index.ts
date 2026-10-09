import { useSyncExternalStore } from 'react';

import type { AppErrorInfo, LanguageCode } from '@/types';
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

/** A dictionary entry for a key built at runtime, or null when none answers. */
function lookUp(key: string): string | null {
  const text = translate(key as TranslationKey);
  return text === key ? null : text;
}

/**
 * The two lines said about a failure the backend reported.
 *
 * The backend writes in English and knows nothing of the chosen language, so
 * its sentences are the fallback rather than the answer: the code is looked up
 * first, and only a code no dictionary answers to falls through to what came
 * back over the wire.
 *
 * A phone may have a sentence of its own (`error.<code>.messageMobile`) where
 * the desktop's advice -- connect a browser, turn on its session -- cannot be
 * followed there. Whether this is a phone is passed in rather than read here:
 * the platform check would bring the IPC layer and the OS plugin into every
 * module and test that only wants a translation.
 */
export function errorCopy(
  error: AppErrorInfo,
  isMobile = false,
): { title: string; message: string } {
  return {
    title: lookUp(`error.${error.code}.title`) ?? error.title,
    message:
      (isMobile ? lookUp(`error.${error.code}.messageMobile`) : null) ??
      lookUp(`error.${error.code}.message`) ??
      error.message,
  };
}

/** What to say about a failure, as `errorCopy` words it, without its title. */
export function errorMessage(error: AppErrorInfo, isMobile = false): string {
  return errorCopy(error, isMobile).message;
}

/**
 * Subscribing to the language store is what makes a language switch repaint
 * instantly, with no reload and no prop drilling.
 */
export function useTranslation() {
  const language = useSyncExternalStore(subscribe, getLanguage, getLanguage);
  return { t: translate, language };
}

/**
 * The first of the system's preferred languages the app speaks, else English.
 * The whole list rather than its first entry: someone whose phone lists German
 * and then Turkish reads Turkish better than the English fallback. Used on the
 * first launch after install, and by a reset (see the settings store).
 */
export function detectSystemLanguage(): LanguageCode {
  const preferred = navigator.languages?.length ? navigator.languages : [navigator.language];
  for (const tag of preferred) {
    const code = tag?.toLowerCase().split('-')[0];
    const known = LANGUAGES.find((language) => language.code === code);
    if (known) return known.code;
  }
  return 'en';
}

export type { TranslationKey };

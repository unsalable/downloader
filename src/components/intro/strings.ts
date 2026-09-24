import type { TranslationKey } from '@/i18n';

/**
 * Every word the film says, looked up once and handed to it as props: the
 * composition is drawn by the app's player and by the render script alike, and
 * only the app has a live language to ask.
 */
const KEYS = {
  summary: 'intro.summary',

  // The film's own words.
  share: 'intro.share',
  saved: 'intro.saved',
  trim: 'intro.trim',
  fast: 'intro.fast',
  private: 'intro.private',
  adFree: 'intro.adFree',
  sampleTitle: 'intro.sampleTitle',
  sampleChannel: 'intro.sampleChannel',

  // The app's own words, for the app drawn inside the film's phone: the same
  // keys the real screens use, so the two can never disagree.
  appName: 'app.name',
  navHome: 'nav.home',
  navDownloads: 'nav.downloads',
  navEditor: 'nav.editor',
  navConvert: 'nav.convert',
  navSettings: 'nav.settings',
  navHistory: 'nav.history',
  heroTitle: 'hero.title',
  heroSubtitle: 'hero.subtitle',
  placeholder: 'input.placeholder',
  paste: 'input.paste',
  analyze: 'input.analyze',
  shareHint: 'input.shareHint',
  mode: 'options.mode',
  modeVideo: 'options.modeVideo',
  modeAudio: 'options.modeAudio',
  quality: 'options.quality',
  qualityBest: 'options.qualityBest',
  format: 'options.format',
  audioBest: 'options.audioBest',
  containerKeep: 'settings.containerKeep',
  download: 'action.download',
  downloadsActive: 'downloads.active',
  merging: 'stage.merging',
  editorLength: 'editor.length',
  clipTab: 'editor.clipTab',
  outputTab: 'editor.outputTab',
  aspect: 'editor.aspect',
  original: 'editor.original',
  pickTitle: 'editor.pickTitle',
  pickBody: 'editor.pickBody',
  chooseVideo: 'editor.chooseVideo',
  fromLink: 'editor.linkTitle',
  exportVideo: 'editor.export',
} as const satisfies Record<string, TranslationKey>;

export type IntroStrings = Record<keyof typeof KEYS, string>;

export function introStrings(lookup: (key: TranslationKey) => string): IntroStrings {
  const entries = Object.entries(KEYS).map(([name, key]) => [name, lookup(key)]);
  return Object.fromEntries(entries) as IntroStrings;
}

import type { DownloadRequest, Handoff, Settings } from '@/types';

type Defaults = Pick<Settings, 'defaultMode' | 'defaultQuality' | 'defaultContainer'>;

/**
 * The request for a video the browser extension handed over: the one Home
 * would send for the link with the default options untouched, since the user
 * picked the video in the browser and there is nothing left to ask them.
 *
 * Nothing is analysed first. The backend analyses the link when the download
 * starts, with the page's headers from `source`, and whatever goes wrong
 * shows on the download's row like any other failure.
 */
export function requestFromHandoff(handoff: Handoff, defaults: Defaults): DownloadRequest {
  const mode = handoff.kind === 'audio' ? 'audio' : defaults.defaultMode;
  return {
    url: handoff.url,
    mode,
    quality: defaults.defaultQuality,
    videoFormatId: null,
    audioFormatId: null,
    // A default container belongs to the default mode, as on Home: an MP4
    // preference would have a song converted into a video.
    container: mode === defaults.defaultMode ? defaults.defaultContainer : null,
    watermark: 'any',
    outputDir: null,
    title: handoff.title?.trim() || null,
    thumbnailUrl: handoff.thumbnail || null,
    // The backend tells the site from the link; a stream's own address rarely
    // says which page it played on.
    platform: null,
    audioLanguage: null,
    source: {
      pageUrl: handoff.pageUrl || null,
      referer: handoff.referer || null,
      origin: handoff.origin || null,
      userAgent: handoff.userAgent || null,
    },
  };
}

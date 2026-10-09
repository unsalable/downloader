import type { DownloadMode, Handoff, QualityPreference, Settings, SourceContext } from '@/types';

type Defaults = Pick<Settings, 'defaultMode' | 'defaultQuality' | 'defaultContainer'>;

/**
 * What a handed-over link brings to Home besides its address: the page it
 * played on and the headers it was fetched under, and what the browser showed
 * of it. Kept beside the link while it is the one in the field, so the
 * analysis and the download both go out the way the player's own requests
 * did.
 */
export interface HandoffContext {
  url: string;
  kind: Handoff['kind'];
  source: SourceContext;
  title: string | null;
  thumbnail: string | null;
}

/** The options Home opens a handed-over link with, and what it carries. */
export interface HandoffOnHome {
  url: string;
  options: { mode: DownloadMode; quality: QualityPreference; container: string | null };
  context: HandoffContext;
}

/**
 * How a video the browser extension handed over opens on Home: analysed, with
 * the default options chosen and every one of them still the user's to change
 * -- the quality, the watermark, the items of a post -- before anything is
 * downloaded. The extension's İndir picks the video; the app is where it is
 * decided how.
 *
 * `Handoff::to_request` in src-tauri/src/bridge/handoff.rs works out the
 * popup's preview from the same defaults, and has to stay in step with these
 * options, or the popup promises one file and Home opens on another.
 */
export function homeFromHandoff(handoff: Handoff, defaults: Defaults): HandoffOnHome {
  const mode = handoff.kind === 'audio' ? 'audio' : defaults.defaultMode;
  return {
    url: handoff.url,
    options: {
      mode,
      quality: defaults.defaultQuality,
      // A default container belongs to the default mode, as on Home: an MP4
      // preference would have a song converted into a video.
      container: mode === defaults.defaultMode ? defaults.defaultContainer : null,
    },
    context: {
      url: handoff.url,
      kind: handoff.kind,
      source: {
        pageUrl: handoff.pageUrl || null,
        referer: handoff.referer || null,
        origin: handoff.origin || null,
        userAgent: handoff.userAgent || null,
      },
      title: handoff.title?.trim() || null,
      thumbnail: handoff.thumbnail || null,
    },
  };
}

/**
 * The name and picture to show for a handed-over link once it is analysed. A
 * site the app knows names its own posts better than a tab title does; a
 * stream or a file the page played is named after its address, so there the
 * browser's title is the better one. A picture the analysis lacks is taken
 * from the browser either way.
 */
export function titledFromHandoff<T extends { title: string; thumbnailUrl: string | null }>(
  metadata: T,
  context: HandoffContext | null,
): T {
  if (!context) return metadata;
  const title = context.kind !== 'page' && context.title ? context.title : metadata.title;
  const thumbnailUrl = metadata.thumbnailUrl ?? context.thumbnail;
  if (title === metadata.title && thumbnailUrl === metadata.thumbnailUrl) return metadata;
  return { ...metadata, title, thumbnailUrl };
}

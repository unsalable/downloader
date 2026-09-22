import { create } from 'zustand';

import { normalizeUrl } from '@/lib/url';
import * as ipc from '@/services/ipc';
import type {
  AppErrorInfo,
  DownloadMode,
  MediaMetadata,
  PlatformId,
  QualityPreference,
  WatermarkPreference,
} from '@/types';

export type AnalysisPhase = 'idle' | 'analyzing' | 'ready' | 'error';

/** The user's pending choices for the media currently on screen. */
export interface DownloadOptions {
  mode: DownloadMode;
  quality: QualityPreference;
  container: string | null;
  watermark: WatermarkPreference;
  videoFormatId: string | null;
  audioFormatId: string | null;
  outputDir: string | null;
  advanced: boolean;
}

interface AnalysisState {
  url: string;
  platform: PlatformId;
  phase: AnalysisPhase;
  metadata: MediaMetadata | null;
  error: AppErrorInfo | null;
  options: DownloadOptions;
  /** A link noticed on the clipboard, offered under the empty field on Home
   *  rather than acted on. Not part of an analysis, so `reset` leaves it be. */
  clipboardSuggestion: string | null;

  setUrl: (url: string) => void;
  setClipboardSuggestion: (url: string | null) => void;
  setPlatform: (platform: PlatformId) => void;
  analyze: (url: string, defaults: Partial<DownloadOptions>) => Promise<void>;
  cancel: () => void;
  reset: () => void;
  setOptions: (patch: Partial<DownloadOptions>) => void;
}

const DEFAULT_OPTIONS: DownloadOptions = {
  mode: 'video',
  quality: { type: 'best' },
  container: null,
  watermark: 'any',
  videoFormatId: null,
  audioFormatId: null,
  outputDir: null,
  advanced: false,
};

/**
 * Guards against a slow earlier analysis overwriting a newer one. Each call
 * takes a token; only the most recent token is allowed to commit a result.
 */
let requestToken = 0;

export const useAnalysisStore = create<AnalysisState>((set, get) => ({
  url: '',
  platform: 'unknown',
  phase: 'idle',
  metadata: null,
  error: null,
  options: DEFAULT_OPTIONS,
  clipboardSuggestion: null,

  // Anything in the field, typed or put there, answers the suggestion.
  setUrl: (url) => set(url ? { url, clipboardSuggestion: null } : { url }),

  setClipboardSuggestion: (suggestion) => {
    // The link already in the field is not news. Without this it would be
    // offered again the moment its own download was queued and the field cleared.
    if (suggestion && suggestion === normalizeUrl(get().url)) return;
    set({ clipboardSuggestion: suggestion });
  },
  setPlatform: (platform) => set({ platform }),

  analyze: async (url, defaults) => {
    const token = ++requestToken;
    set({
      url,
      clipboardSuggestion: null,
      phase: 'analyzing',
      error: null,
      metadata: null,
      options: { ...DEFAULT_OPTIONS, ...defaults },
    });

    try {
      const metadata = await ipc.analyzeUrl(url);
      if (token !== requestToken) return;

      set((state) => ({
        phase: 'ready',
        metadata,
        platform: metadata.platform,
        options: reconcileOptions(state.options, metadata),
      }));
    } catch (error) {
      if (token !== requestToken) return;
      set({ phase: 'error', error: ipc.toAppError(error) });
    }
  },

  cancel: () => {
    // The Rust side has no cancel for a single analyze call (it is one short
    // subprocess), so the result is dropped by invalidating the token.
    requestToken += 1;
    set({ phase: 'idle', metadata: null, error: null });
  },

  reset: () => {
    requestToken += 1;
    set({
      url: '',
      platform: 'unknown',
      phase: 'idle',
      metadata: null,
      error: null,
      options: DEFAULT_OPTIONS,
    });
  },

  setOptions: (patch) => {
    const next = { ...get().options, ...patch };

    // Switching mode invalidates an explicit stream choice made for the old one.
    if (patch.mode && patch.mode !== get().options.mode) {
      next.videoFormatId = null;
      next.audioFormatId = null;
      next.container = null;
    }
    set({ options: next });
  },
}));

/**
 * Fold what the source actually offers into the user's defaults, so the panel
 * never opens on an option this particular media cannot satisfy.
 */
function reconcileOptions(options: DownloadOptions, metadata: MediaMetadata): DownloadOptions {
  const next = { ...options };

  const hasVideo = metadata.formats.some((format) => format.hasVideo);
  const hasAudio = metadata.formats.some((format) => format.hasAudio);
  const hasImage = metadata.formats.some((format) => format.kind === 'image');

  if (next.mode === 'video' && !hasVideo) {
    // A photo post may come with a soundtrack, but the photos are the post.
    next.mode = hasImage ? 'image' : 'audio';
  } else if (next.mode === 'audio' && !hasAudio) {
    next.mode = hasVideo ? 'video' : 'image';
  } else if (next.mode === 'image' && !hasImage) {
    next.mode = hasVideo ? 'video' : 'audio';
  }

  // A default container belongs to the default mode: an MP4 preference means
  // nothing for a photo, and would otherwise try to "convert" one into video.
  if (next.mode !== options.mode) {
    next.container = null;
  }

  // Only offer "without watermark" where a clean rendition genuinely exists.
  if (metadata.watermarkSupport !== 'cleanAvailable') {
    next.watermark = 'any';
  }

  return next;
}

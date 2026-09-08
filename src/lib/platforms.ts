import type { PlatformId } from '@/types';

export interface PlatformPresentation {
  label: string;
  /** Brand-adjacent hue used for the badge tint. */
  color: string;
  /** Monogram shown in the badge. Deliberately not the platform's logo: the
   *  app identifies sources without reproducing anyone's trademarked mark. */
  monogram: string;
}

const PRESENTATION: Record<PlatformId, PlatformPresentation> = {
  youtube: { label: 'YouTube', color: '#FF0033', monogram: 'YT' },
  tiktok: { label: 'TikTok', color: '#25F4EE', monogram: 'TT' },
  instagram: { label: 'Instagram', color: '#E1306C', monogram: 'IG' },
  twitter: { label: 'X', color: '#8E8EA0', monogram: 'X' },
  reddit: { label: 'Reddit', color: '#FF4500', monogram: 'RD' },
  facebook: { label: 'Facebook', color: '#1877F2', monogram: 'FB' },
  twitch: { label: 'Twitch', color: '#9146FF', monogram: 'TW' },
  pinterest: { label: 'Pinterest', color: '#E60023', monogram: 'PN' },
  vimeo: { label: 'Vimeo', color: '#17D5FF', monogram: 'VM' },
  dailymotion: { label: 'Dailymotion', color: '#0066DC', monogram: 'DM' },
  soundcloud: { label: 'SoundCloud', color: '#FF5500', monogram: 'SC' },
  direct: { label: 'Direct file', color: '#6366F1', monogram: '::' },
  generic: { label: 'Web page', color: '#8B8B9C', monogram: 'WW' },
  unknown: { label: 'Unknown', color: '#8B8B9C', monogram: '?' },
};

export function platformPresentation(id: PlatformId): PlatformPresentation {
  return PRESENTATION[id] ?? PRESENTATION.unknown;
}

/**
 * The platforms advertised on the Home screen. Kept short and honest: these are
 * the sources the shipped provider set is verified against, not a wish list.
 */
export const FEATURED_PLATFORMS: PlatformId[] = [
  'youtube',
  'tiktok',
  'instagram',
  'twitter',
  'reddit',
  'twitch',
  'vimeo',
  'soundcloud',
];

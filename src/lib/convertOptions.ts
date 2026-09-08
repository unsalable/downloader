/**
 * Option lists for the converter.
 *
 * Everything here is pure and derived from the catalogue the backend serves --
 * the set of formats is never written down a second time in TypeScript, so the
 * list on screen cannot offer a target the backend would refuse.
 */

import type { ConvertFormatInfo, ConvertKind } from '@/types';

/** Heights offered as a downscale cap, largest first. */
export const RESOLUTION_CAPS = [2160, 1440, 1080, 720, 480, 360] as const;

/** Audio bitrates offered, in kbps. */
export const AUDIO_BITRATES = [320, 256, 192, 160, 128, 96] as const;

/** Containers that carry uncompressed or lossless audio, where a bitrate is
 *  meaningless and is therefore not offered. */
const FIXED_RATE_FORMATS = new Set(['wav', 'flac']);

export function formatsOfKind(catalogue: ConvertFormatInfo[], kind: ConvertKind): string[] {
  return catalogue.filter((entry) => entry.kind === kind).map((entry) => entry.id);
}

export function kindOf(catalogue: ConvertFormatInfo[], target: string): ConvertKind | null {
  return catalogue.find((entry) => entry.id === target)?.kind ?? null;
}

export function acceptsBitrate(target: string): boolean {
  return !FIXED_RATE_FORMATS.has(target);
}

/**
 * Input extensions offered in the file picker. Deliberately wider than the list
 * of targets: plenty of formats can be read that it makes no sense to write.
 */
export const INPUT_EXTENSIONS = [
  'mp4',
  'mkv',
  'webm',
  'mov',
  'avi',
  'm4v',
  'flv',
  'wmv',
  'mpg',
  'mpeg',
  'ts',
  'm2ts',
  '3gp',
  'ogv',
  'mp3',
  'm4a',
  'aac',
  'wav',
  'flac',
  'opus',
  'ogg',
  'oga',
  'weba',
  'wma',
  'aiff',
  'aif',
  'mka',
] as const;

/** "1920x1080" when both sides are known, otherwise whichever one is. */
export function formatResolution(
  width: number | null,
  height: number | null,
): string | null {
  if (width != null && height != null) return `${width}x${height}`;
  if (height != null) return `${height}p`;
  return null;
}

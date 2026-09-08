import type { DropdownOption } from '@/components/ui/Dropdown';
import { translate } from '@/i18n';
import { formatBitrate, formatBytes, prettyCodec } from '@/lib/format';
import type { DownloadMode, MediaFormat, MediaMetadata, QualityPreference } from '@/types';

/**
 * Derives the choices offered for a given piece of media.
 *
 * This decides what is *shown*; which stream actually gets downloaded is
 * decided by `plan::build` in Rust, and the panel asks that same code for the
 * resulting label and size (see `summarizePlan`). Only the source's real
 * renditions appear here -- no aspirational "4K" entry for a 720p video.
 */

export type QualityKey = string;

export function encodeQuality(quality: QualityPreference): QualityKey {
  switch (quality.type) {
    case 'best':
      return 'best';
    case 'auto':
      return 'auto';
    case 'maxHeight':
      return `h:${quality.height}`;
    case 'audioBitrate':
      return `a:${quality.kbps}`;
  }
}

export function decodeQuality(key: QualityKey): QualityPreference {
  if (key === 'best') return { type: 'best' };
  if (key === 'auto') return { type: 'auto' };
  if (key.startsWith('h:')) return { type: 'maxHeight', height: Number(key.slice(2)) };
  if (key.startsWith('a:')) return { type: 'audioBitrate', kbps: Number(key.slice(2)) };
  return { type: 'best' };
}

export function availableModes(metadata: MediaMetadata): DownloadMode[] {
  const modes: DownloadMode[] = [];
  if (metadata.formats.some((format) => format.hasVideo)) modes.push('video');
  if (metadata.formats.some((format) => format.hasAudio)) modes.push('audio');
  if (metadata.formats.some((format) => format.kind === 'image')) modes.push('image');
  return modes.length > 0 ? modes : ['video'];
}

export function qualityOptions(
  metadata: MediaMetadata,
  mode: DownloadMode,
): DropdownOption<QualityKey>[] {
  if (mode === 'image') {
    return [{ value: 'best', label: translate('options.qualityBest') }];
  }

  if (mode === 'audio') {
    const bitrates = [
      ...new Set(
        metadata.formats
          .filter((format) => format.kind === 'audio' && format.abr != null)
          .map((format) => Math.round(format.abr!)),
      ),
    ].sort((a, b) => b - a);

    return [
      { value: 'best', label: translate('options.audioBest') },
      ...bitrates.map((kbps) => ({
        value: `a:${kbps}` satisfies QualityKey,
        label: `${kbps} kbps`,
      })),
    ];
  }

  // Distinct heights, best first. A source that reports no height at all still
  // gets Best/Auto rather than an empty menu.
  const heights = [
    ...new Set(
      metadata.formats
        .filter((format) => format.hasVideo && format.height != null)
        .map((format) => format.height!),
    ),
  ].sort((a, b) => b - a);

  const options: DropdownOption<QualityKey>[] = [
    {
      value: 'best',
      label: translate('options.qualityBest'),
      description: translate('options.qualityBestHint'),
    },
    { value: 'auto', label: translate('options.qualityAuto') },
  ];

  for (const height of heights) {
    const best = metadata.formats
      .filter((format) => format.height === height && format.hasVideo)
      .sort((a, b) => (b.tbr ?? 0) - (a.tbr ?? 0))[0];

    options.push({
      value: `h:${height}`,
      label: `${height}p`,
      description: describeStream(best),
      meta: best?.filesize != null ? formatBytes(best.filesize) : undefined,
    });
  }

  return options;
}

const VIDEO_CONTAINERS = ['mp4', 'webm', 'mkv'];
const AUDIO_CONTAINERS = ['mp3', 'm4a', 'wav', 'opus'];
const IMAGE_CONTAINERS = ['jpg', 'png', 'webp'];

export function containerOptions(
  mode: DownloadMode,
  ffmpegAvailable: boolean,
): DropdownOption<string>[] {
  const list =
    mode === 'audio' ? AUDIO_CONTAINERS : mode === 'image' ? IMAGE_CONTAINERS : VIDEO_CONTAINERS;

  return [
    { value: '', label: translate('settings.containerKeep') },
    ...list.map((container) => ({
      value: container,
      label: container.toUpperCase(),
      // Changing container is an FFmpeg job; offering it without FFmpeg would
      // be a button that cannot work.
      disabled: !ffmpegAvailable,
      disabledReason: !ffmpegAvailable ? translate('error.ffmpegMissing.message') : undefined,
    })),
  ];
}

/** "H.264 · 30 fps · 4.2 Mbps" for the advanced stream pickers. */
export function describeStream(format: MediaFormat | undefined): string | undefined {
  if (!format) return undefined;

  const parts: string[] = [];
  const codec = prettyCodec(format.vcodec ?? format.acodec);
  if (codec) parts.push(codec);
  if (format.fps != null && format.fps > 0) parts.push(`${Math.round(format.fps)} fps`);

  const bitrate = formatBitrate(format.vbr ?? format.abr ?? format.tbr);
  if (bitrate) parts.push(bitrate);
  parts.push(format.container.toUpperCase());

  return parts.join(' · ');
}

export function videoStreamOptions(metadata: MediaMetadata): DropdownOption<string>[] {
  return metadata.formats
    .filter((format) => format.hasVideo)
    .map((format) => ({
      value: format.id,
      label: `${format.qualityLabel}${format.hasAudio ? ' + audio' : ''}`,
      description: describeStream(format),
      meta: sizeLabel(format),
    }));
}

export function audioStreamOptions(metadata: MediaMetadata): DropdownOption<string>[] {
  const options = metadata.formats
    .filter((format) => format.kind === 'audio')
    .map((format) => ({
      value: format.id,
      label: format.qualityLabel,
      description: describeStream(format),
      meta: sizeLabel(format),
    }));

  return [{ value: '', label: translate('options.none') }, ...options];
}

function sizeLabel(format: MediaFormat): string | undefined {
  const size = format.filesize ?? format.filesizeApprox;
  if (size == null) return undefined;
  return format.filesize == null ? `~${formatBytes(size)}` : formatBytes(size);
}

/** Whether the watermark control is meaningful for this source. */
export function watermarkState(
  metadata: MediaMetadata,
): 'hidden' | 'available' | 'unavailable' {
  switch (metadata.watermarkSupport) {
    case 'cleanAvailable':
      return 'available';
    case 'watermarkedOnly':
      return 'unavailable';
    default:
      return 'hidden';
  }
}

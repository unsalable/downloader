import type { DropdownOption } from '@/components/ui/Dropdown';
import { getLanguage, translate } from '@/i18n';
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

/**
 * The languages a video's sound comes in, when it comes in more than one: the
 * original first, as the empty value that means "the original", then the
 * dubs by name.
 */
export function audioLanguageOptions(metadata: MediaMetadata): DropdownOption<string>[] {
  const tracks = metadata.formats.filter((format) => format.kind === 'audio' && format.language);
  const codes = [...new Set(tracks.map((format) => format.language!))];
  if (codes.length < 2) return [];

  const top = Math.max(...tracks.map((format) => format.languagePreference ?? -Infinity));
  const original =
    Number.isFinite(top) && tracks.filter((format) => format.languagePreference === top).length < tracks.length
      ? tracks.find((format) => format.languagePreference === top)?.language
      : undefined;

  const names = displayNames();
  const name = (code: string) => {
    const shown = names?.of(code) ?? code;
    return shown.charAt(0).toLocaleUpperCase(getLanguage()) + shown.slice(1);
  };

  const options: DropdownOption<string>[] = [
    {
      value: '',
      label: original
        ? translate('options.audioOriginalNamed', { name: name(original) })
        : translate('options.audioOriginal'),
    },
  ];
  for (const code of codes.sort((a, b) => name(a).localeCompare(name(b), getLanguage()))) {
    if (code === original) continue;
    options.push({ value: code, label: name(code) });
  }
  return options;
}

function displayNames(): Intl.DisplayNames | null {
  try {
    return new Intl.DisplayNames([getLanguage()], { type: 'language' });
  } catch {
    return null;
  }
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

/**
 * The picture menu of the advanced panel. `chosen` is the picture the plan
 * took (or the one picked by hand); see `onePerRendition`.
 */
export function videoStreamOptions(
  metadata: MediaMetadata,
  chosen?: string | null,
): DropdownOption<string>[] {
  return onePerRendition(
    metadata.formats.filter((format) => format.hasVideo),
    (format) => ({
      value: format.id,
      label: format.hasAudio
        ? translate('options.withSound', { quality: format.qualityLabel })
        : format.qualityLabel,
      description: describeStream(format),
      meta: sizeLabel(format),
    }),
    chosen,
  );
}

/**
 * The sound menu of the advanced panel. `chosen` is the sound the plan took
 * (or the one picked by hand), so that the copy it names is the copy listed:
 * the menu otherwise has nothing to show for it but a bare "--".
 */
export function audioStreamOptions(
  metadata: MediaMetadata,
  chosen?: string | null,
): DropdownOption<string>[] {
  const apart = metadata.formats.filter((format) => format.kind === 'audio');
  const options =
    apart.length > 0
      ? apart.map((format) => ({
          value: format.id,
          label: format.qualityLabel,
          description: describeStream(format),
          meta: sizeLabel(format),
        }))
      : borrowedSoundOptions(metadata, chosen);

  return [{ value: '', label: translate('options.none') }, ...options];
}

/**
 * The sound of each rendition that carries one, for a source that keeps no
 * sound apart.
 *
 * TikTok publishes its sharpest picture without sound and its sound only
 * inside another rendition, and the plan takes it from there (see
 * `sound_donor` in plan.rs); without these the menu shows a bare "--" for
 * that choice, and has nothing to offer someone who picks the picture by
 * hand. Only renditions that name their sound codec, as the plan requires,
 * and a stamped one only when nothing clean has sound -- but what the plan
 * took is always listed, and once per rendition (see `onePerRendition`).
 *
 * Nothing at all where every picture brings its own sound and none was taken
 * out of one: lent to a picture that already has sound, it is dropped, and an
 * entry for it would be a choice that changes nothing.
 */
function borrowedSoundOptions(
  metadata: MediaMetadata,
  chosen: string | null | undefined,
): DropdownOption<string>[] {
  const muxed = metadata.formats.filter((format) => format.kind === 'muxed' && format.hasAudio);
  const picked = muxed.find((format) => format.id === chosen);
  if (!picked && !metadata.formats.some((format) => format.kind === 'video')) return [];

  const named = muxed.filter((format) => format.acodec != null && format.acodec !== 'none');
  const clean = named.filter((format) => format.watermarked !== true);
  const pool = [...(clean.length > 0 ? clean : named)];
  if (picked && !pool.includes(picked)) pool.push(picked);

  return onePerRendition(
    pool,
    (format) => ({
      value: format.id,
      label: translate('options.soundOf', { quality: format.qualityLabel }),
      description: describeSound(format),
      meta: sizeLabel(format),
    }),
    chosen,
  );
}

/**
 * One entry for each choice that reads differently. TikTok lists every
 * rendition twice, once per CDN, and two entries that say the same thing are
 * one choice to the person reading them. The copy kept is `chosen` when it is
 * one of them, so the menu shows what the plan took rather than a bare "--";
 * otherwise the last listed, which is the copy the plan takes itself.
 */
function onePerRendition(
  formats: MediaFormat[],
  toOption: (format: MediaFormat) => DropdownOption<string>,
  chosen: string | null | undefined,
): DropdownOption<string>[] {
  const kept = new Map<string, DropdownOption<string>>();
  for (const format of formats) {
    const option = toOption(format);
    // What the entry says, and what it carries without saying: two entries
    // that differ only in the language of their sound are still two choices.
    const key = [
      option.label,
      option.description,
      option.meta,
      format.language,
      format.note,
      format.protocol,
    ].join('|');
    const held = kept.get(key);
    if (held == null || held.value !== chosen) kept.set(key, option);
  }
  return [...kept.values()];
}

/** "AAC · 128 kbps": the sound of a rendition, without its picture. */
function describeSound(format: MediaFormat): string | undefined {
  const parts = [prettyCodec(format.acodec), formatBitrate(format.abr)].filter(
    (part): part is string => part != null,
  );
  return parts.length > 0 ? parts.join(' · ') : undefined;
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

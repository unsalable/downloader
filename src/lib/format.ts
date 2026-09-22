/**
 * Display formatters. Everything here is pure so it can be unit tested without
 * a DOM, and every numeric readout is padded for tabular rendering rather than
 * changing width as digits change.
 */

const BYTE_UNITS = ['B', 'KB', 'MB', 'GB', 'TB'] as const;

/** 1 234 567 -> "1.18 MB". Uses binary units, which is what disks report. */
export function formatBytes(bytes: number | null | undefined, digits?: number): string {
  if (bytes == null || !Number.isFinite(bytes) || bytes < 0) return '--';
  if (bytes < 1) return '0 B';

  let value = bytes;
  let unit = 0;
  while (value >= 1024 && unit < BYTE_UNITS.length - 1) {
    value /= 1024;
    unit += 1;
  }

  // Bytes and kilobytes read better without decimals; larger units need one or
  // two so a slowly growing number still visibly moves.
  const precision = digits ?? (unit <= 1 ? 0 : value >= 100 ? 0 : value >= 10 ? 1 : 2);
  return `${value.toFixed(precision)} ${BYTE_UNITS[unit]}`;
}

export function formatSpeed(bytesPerSecond: number | null | undefined): string {
  if (bytesPerSecond == null || !Number.isFinite(bytesPerSecond) || bytesPerSecond <= 0) {
    return '--';
  }
  return `${formatBytes(bytesPerSecond, bytesPerSecond >= 1024 * 1024 * 100 ? 0 : 1)}/s`;
}

/** Seconds -> "01:42" or "1:02:03". */
export function formatDuration(totalSeconds: number | null | undefined): string {
  if (totalSeconds == null || !Number.isFinite(totalSeconds) || totalSeconds < 0) return '--';

  const seconds = Math.floor(totalSeconds % 60);
  const minutes = Math.floor((totalSeconds / 60) % 60);
  const hours = Math.floor(totalSeconds / 3600);

  const mm = hours > 0 ? String(minutes).padStart(2, '0') : String(minutes);
  const ss = String(seconds).padStart(2, '0');
  return hours > 0 ? `${hours}:${mm}:${ss}` : `${mm}:${ss}`;
}

/**
 * A position in a file, to a tenth of a second.
 *
 * `formatDuration` rounds to whole seconds, which is right for "how long is
 * this" and wrong for a mark the user is placing: two marks a frame apart would
 * read as the same number, and the readout would sit still while the handle
 * moved. Minutes are always padded here, because this is read while it changes
 * and a field that grows a digit shifts everything beside it.
 */
export function formatTimecode(totalSeconds: number | null | undefined): string {
  if (totalSeconds == null || !Number.isFinite(totalSeconds) || totalSeconds < 0) return '--';

  // Rounded once, up front: rounding the parts separately turns 59.97 into
  // "0:60.0".
  const tenths = Math.round(totalSeconds * 10);
  const whole = Math.floor(tenths / 10);
  const tenth = tenths % 10;

  const seconds = whole % 60;
  const minutes = Math.floor(whole / 60) % 60;
  const hours = Math.floor(whole / 3600);

  const mm = String(minutes).padStart(2, '0');
  const ss = String(seconds).padStart(2, '0');
  return hours > 0 ? `${hours}:${mm}:${ss}.${tenth}` : `${mm}:${ss}.${tenth}`;
}

/**
 * ETA gets its own formatter: past an hour the exact seconds are noise, and a
 * missing estimate should read as unknown rather than "00:00".
 */
export function formatEta(seconds: number | null | undefined): string {
  if (seconds == null || !Number.isFinite(seconds) || seconds < 0) return '--';
  if (seconds >= 3600 * 24) return '> 1d';
  return formatDuration(Math.round(seconds));
}

export function formatCount(value: number | null | undefined): string {
  if (value == null || !Number.isFinite(value)) return '--';
  if (value < 1000) return String(value);
  if (value < 1_000_000) return `${(value / 1000).toFixed(value < 10_000 ? 1 : 0)}K`;
  if (value < 1_000_000_000) return `${(value / 1_000_000).toFixed(value < 10_000_000 ? 1 : 0)}M`;
  return `${(value / 1_000_000_000).toFixed(1)}B`;
}

export function formatDate(timestampMs: number, locale: string): string {
  const date = new Date(timestampMs);
  if (Number.isNaN(date.getTime())) return '--';
  return new Intl.DateTimeFormat(locale, {
    year: 'numeric',
    month: 'short',
    day: 'numeric',
    hour: '2-digit',
    minute: '2-digit',
  }).format(date);
}

/**
 * The day alone, for a row too narrow for `formatDate`: "21 Sep", and the year
 * only once it is no longer this one.
 */
export function formatDay(timestampMs: number, locale: string): string {
  const date = new Date(timestampMs);
  if (Number.isNaN(date.getTime())) return '--';
  const sameYear = date.getFullYear() === new Date().getFullYear();
  return new Intl.DateTimeFormat(locale, {
    year: sameYear ? undefined : 'numeric',
    month: 'short',
    day: 'numeric',
  }).format(date);
}

/** "2026-09-07" or "20260907" as reported by sources -> localized short date. */
export function formatUploadDate(raw: string | null, locale: string): string | null {
  if (!raw) return null;
  const compact = /^(\d{4})(\d{2})(\d{2})$/.exec(raw);
  const iso = compact ? `${compact[1]}-${compact[2]}-${compact[3]}` : raw;
  const date = new Date(iso);
  if (Number.isNaN(date.getTime())) return null;
  return new Intl.DateTimeFormat(locale, { year: 'numeric', month: 'short', day: 'numeric' })
    .format(date);
}

export function formatBitrate(kbps: number | null | undefined): string | null {
  if (kbps == null || !Number.isFinite(kbps) || kbps <= 0) return null;
  return `${Math.round(kbps)} kbps`;
}

/** Strips the vendor detail off a codec id: "avc1.640028" -> "H.264". */
export function prettyCodec(codec: string | null | undefined): string | null {
  if (!codec || codec === 'none') return null;
  const base = codec.split('.')[0]?.toLowerCase() ?? '';
  const map: Record<string, string> = {
    avc1: 'H.264',
    avc3: 'H.264',
    h264: 'H.264',
    hev1: 'H.265',
    hvc1: 'H.265',
    h265: 'H.265',
    vp9: 'VP9',
    'vp09': 'VP9',
    vp8: 'VP8',
    av01: 'AV1',
    mp4a: 'AAC',
    aac: 'AAC',
    opus: 'Opus',
    vorbis: 'Vorbis',
    mp3: 'MP3',
    flac: 'FLAC',
    'ec-3': 'E-AC-3',
    'ac-3': 'AC-3',
  };
  return map[base] ?? codec.split('.')[0]?.toUpperCase() ?? null;
}

/** Clamp a percentage into 0..100 and round to a tenth for smooth bars. */
export function clampPercent(value: number | null | undefined): number {
  if (value == null || !Number.isFinite(value)) return 0;
  return Math.min(100, Math.max(0, Math.round(value * 10) / 10));
}

export function truncateMiddle(text: string, max: number): string {
  if (text.length <= max) return text;
  const half = Math.floor((max - 1) / 2);
  return `${text.slice(0, half)}…${text.slice(text.length - half)}`;
}

/** Last path segment of a Windows or POSIX path. */
export function basename(path: string): string {
  const parts = path.split(/[\\/]/);
  return parts[parts.length - 1] || path;
}

export function dirname(path: string): string {
  const index = Math.max(path.lastIndexOf('\\'), path.lastIndexOf('/'));
  return index > 0 ? path.slice(0, index) : path;
}

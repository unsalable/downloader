import type { MediaProbe } from '@/types';

/*
 * The arithmetic behind the editor's custom bitrate: reading the number the
 * user typed, writing one back, and guessing what the source was made at.
 *
 * The field speaks Mbps because that is the unit people have seen on a camera
 * or an upload page; the export speaks kbps because that is FFmpeg's. Nothing
 * outside this file converts between the two.
 */

/** Below this a video is a slideshow; above it no encoder the app drives goes. */
export const MIN_MBPS = 0.1;
export const MAX_MBPS = 300;

/** When the source gives nothing to go on: a sound rate for 1080p. */
export const FALLBACK_KBPS = 8000;

/**
 * A typed rate, in kbps -- or null when it is not one the export can take.
 *
 * Either decimal mark is read, because Turkish writes 2,5 where English writes
 * 2.5 and people type what they write. A trailing mark ("2.") is the middle of
 * typing "2.5" and counts as 2, so the value never flickers invalid mid-word.
 */
export function parseMbps(text: string): number | null {
  const normalised = text.trim().replace(',', '.');
  if (!/^(\d+\.?\d*|\.\d+)$/.test(normalised)) return null;
  const mbps = Number(normalised);
  if (!Number.isFinite(mbps) || mbps < MIN_MBPS || mbps > MAX_MBPS) return null;
  return Math.round(mbps * 1000);
}

/** A rate in kbps as the user would write it in Mbps: "8", "2.5", or "2,5". */
export function formatMbps(kbps: number, language: string): string {
  return new Intl.NumberFormat(language, {
    maximumFractionDigits: 2,
    useGrouping: false,
  }).format(kbps / 1000);
}

/**
 * A rate rounded the way a person would say it: 164.6 is "165", 8.4 is "8.5",
 * 0.83 is "0.8". The prefill is a starting point to edit, and 8.37 invites the
 * question of where the .37 came from.
 */
export function roundMbps(mbps: number): number {
  const step = mbps < 1 ? 0.1 : mbps < 10 ? 0.5 : mbps < 100 ? 1 : 5;
  return Number((Math.round(mbps / step) * step).toFixed(1));
}

/**
 * What the source's picture was probably encoded at, in kbps.
 *
 * The file's size over its length is the rate of everything in it, so the
 * audio's own rate comes off the top; the container's few bytes of overhead
 * are left in, which is well inside the rounding. A file that gives no size or
 * no length, or whose audio claims more than the whole, falls back to a rate
 * that is reasonable for most of what people cut.
 */
export function sourceVideoKbps(
  probe: Pick<MediaProbe, 'sizeBytes' | 'durationSec' | 'hasAudio' | 'audioBitrateKbps'>,
): number {
  const { sizeBytes, durationSec } = probe;
  if (!durationSec || durationSec <= 0 || !(sizeBytes > 0)) return FALLBACK_KBPS;
  const totalKbps = (sizeBytes * 8) / durationSec / 1000;
  const audioKbps = probe.hasAudio ? (probe.audioBitrateKbps ?? 0) : 0;
  const videoKbps = totalKbps - audioKbps;
  if (!Number.isFinite(videoKbps) || videoKbps <= 0) return FALLBACK_KBPS;
  const mbps = Math.min(MAX_MBPS, Math.max(MIN_MBPS, roundMbps(videoKbps / 1000)));
  return Math.round(mbps * 1000);
}

/** Bytes that a stream of this many kbps writes over this many seconds. */
export function estimatedBytes(kbps: number, seconds: number): number {
  return ((kbps * 1000) / 8) * Math.max(0, seconds);
}

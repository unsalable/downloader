import { memo, useCallback, useEffect, useMemo, useRef } from 'react';

import { useTranslation } from '@/i18n';
import { cn } from '@/lib/cn';
import type { WaveformData } from '@/types';

/**
 * The audio track of the editor's timeline: one canvas painting the peaks that
 * FFmpeg measured on the Rust side.
 *
 * It is deliberately not drawn from the <video> element. The picture element
 * will not hand over its audio at all, and a file the window cannot decode
 * still has to get a usable timeline -- the peaks come off the file itself, so
 * the track is there whether or not anything can play.
 *
 * It is also deliberately not interactive and holds nothing. It takes the
 * window to show, draws it, and lets the surface above it own the pointer, the
 * playhead and the marks; that is what keeps a component repainted on every
 * frame of a scrub from re-rendering anything else. Memoised for the same
 * reason in the other direction: nothing about playback reaches its props.
 */

/**
 * Silence, as the backend writes it. The bytes are unsigned, so an undisturbed
 * sample sits in the middle of the range rather than at zero.
 */
const SILENCE = 128;

/** The most a sample can travel from silence, which is what the bar is scaled by. */
const RANGE = 127;

interface WaveformProps {
  data: WaveformData | null;
  /** The left edge of what is visible, in seconds from the start of the file. */
  windowStartSec: number;
  /** The right edge, in the same units. */
  windowEndSec: number;
  /** The visible width of the track in CSS pixels, which the parent measures. */
  width: number;
  height: number;
  /** A newer window has been asked for; what is already drawn stays up until it lands. */
  loading?: boolean;
  /** The file carries no audio at all, which is not the same as not having it yet. */
  empty?: boolean;
  className?: string;
}

/**
 * Base64 to bytes, once per payload.
 *
 * Doing this inside the paint would spend the whole frame budget on a strip
 * that has not changed: a minute of audio is a few thousand buckets, and a
 * scrub asks for a repaint sixty times a second.
 */
function decodePeaks(peaks: string): Uint8Array | null {
  try {
    const binary = atob(peaks);
    const bytes = new Uint8Array(binary.length);
    for (let i = 0; i < binary.length; i += 1) bytes[i] = binary.charCodeAt(i);
    return bytes;
  } catch {
    // A payload that is not base64 is a backend fault, and losing the audio
    // track is a far smaller failure than taking the editor down with it.
    return null;
  }
}

export const Waveform = memo(function Waveform({
  data,
  windowStartSec,
  windowEndSec,
  width,
  height,
  loading = false,
  empty = false,
  className,
}: WaveformProps) {
  const { t } = useTranslation();
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const frame = useRef<number | null>(null);

  const peaks = data?.peaks ?? null;
  const samples = useMemo(() => (peaks ? decodePeaks(peaks) : null), [peaks]);

  const paint = useCallback(() => {
    const canvas = canvasRef.current;
    if (!canvas || width <= 0 || height <= 0) return;

    const ratio = window.devicePixelRatio || 1;
    const deviceWidth = Math.max(1, Math.round(width * ratio));
    const deviceHeight = Math.max(1, Math.round(height * ratio));
    if (canvas.width !== deviceWidth) canvas.width = deviceWidth;
    if (canvas.height !== deviceHeight) canvas.height = deviceHeight;

    const ctx = canvas.getContext('2d');
    if (!ctx) return;

    // The backing store is in device pixels and the geometry below is in CSS
    // pixels; without this the whole track is drawn at a quarter size on a
    // Retina panel, and stretching it back is what makes a waveform look soft.
    ctx.setTransform(ratio, 0, 0, ratio, 0, 0);
    ctx.clearRect(0, 0, width, height);

    // The theme is a class on <html>, so the token behind `text-accent` has a
    // different value after a swap. Reading it here rather than holding it
    // means the colour can never be a theme behind what is on screen.
    ctx.fillStyle = getComputedStyle(canvas).color;

    // One device pixel, expressed in the units the context is now scaled to.
    const hairline = 1 / ratio;
    const centre = height / 2;

    if (empty) {
      // No audio in this file. A flat line says the track exists and is
      // silent; it is drawn faint because it is a placeholder and not a
      // measurement, and a full-strength line would read as a signal.
      ctx.globalAlpha = 0.35;
      ctx.fillRect(0, centre - hairline / 2, width, hairline);
      ctx.globalAlpha = 1;
      return;
    }

    // Nothing measured yet. The surface underneath is the parent's to fill --
    // it is the one that knows whether this is a wait or a failure.
    if (!data || !samples) return;

    const span = windowEndSec - windowStartSec;
    if (span <= 0 || data.lengthSec <= 0 || data.buckets <= 0) return;

    // Trust the payload's length over its stated count: a short read would
    // otherwise be drawn as a tail of silence that is really missing data.
    const buckets = Math.min(data.buckets, samples.length >> 1);
    if (buckets <= 0) return;

    const dataEnd = data.startSec + data.lengthSec;
    const perSecond = data.buckets / data.lengthSec;
    // Peaks touching the edge of the track look clipped whether or not they
    // are, so the tallest bar stops a hairline short of it.
    const reach = Math.max(hairline, centre - hairline);

    ctx.beginPath();
    for (let column = 0; column < deviceWidth; column += 1) {
      const from = windowStartSec + (column / deviceWidth) * span;
      const to = windowStartSec + ((column + 1) / deviceWidth) * span;
      // A window wider than the file has columns that no bucket answers for,
      // and an empty column is the honest drawing of that.
      if (to <= data.startSec || from >= dataEnd) continue;

      // The extremes across every bucket the column covers, not the one bucket
      // it starts on: sampling one per column drops the transients as the user
      // zooms out, which is exactly where a waveform earns its place.
      let first = Math.floor((from - data.startSec) * perSecond);
      let last = Math.ceil((to - data.startSec) * perSecond) - 1;
      if (first < 0) first = 0;
      if (last > buckets - 1) last = buckets - 1;
      if (last < first) last = first;
      if (first > buckets - 1) continue;

      let low = SILENCE;
      let high = SILENCE;
      for (let bucket = first; bucket <= last; bucket += 1) {
        const bottom = samples[bucket * 2] ?? SILENCE;
        const top = samples[bucket * 2 + 1] ?? SILENCE;
        if (bottom < low) low = bottom;
        if (top > high) high = top;
      }

      const y = centre - ((high - SILENCE) / RANGE) * reach;
      const tall = Math.max(hairline, ((high - low) / RANGE) * reach);
      ctx.rect(column / ratio, y, hairline, tall);
    }
    // One path and one fill. A fillRect per column is a state change per
    // column, and a wide track at two device pixels to the CSS pixel is a few
    // thousand of them a frame.
    ctx.fill();
  }, [data, empty, height, samples, width, windowEndSec, windowStartSec]);

  const schedule = useCallback(() => {
    if (frame.current != null) return;
    // Several things can invalidate the track within one frame -- a scroll
    // moves the window while the parent's resize observer reports a new width
    // -- and each of them paints the same picture.
    frame.current = requestAnimationFrame(() => {
      frame.current = null;
      paint();
    });
  }, [paint]);

  useEffect(() => {
    schedule();
    return () => {
      if (frame.current != null) {
        cancelAnimationFrame(frame.current);
        frame.current = null;
      }
    };
  }, [schedule]);

  useEffect(() => {
    // Canvas keeps no relationship to the stylesheet once it has painted, so a
    // theme swap leaves the old colour on screen until something else happens
    // to redraw it. The swap is a class on the root element and nothing else.
    const observer = new MutationObserver(schedule);
    observer.observe(document.documentElement, {
      attributes: true,
      attributeFilter: ['class'],
    });
    return () => observer.disconnect();
  }, [schedule]);

  return (
    <canvas
      ref={canvasRef}
      role="img"
      aria-label={t('editor.audioTrack')}
      aria-busy={loading}
      style={{ width, height }}
      // Transparent to the pointer: the track underneath is what a press on the
      // timeline means, and a canvas swallowing it would be a dead strip.
      // Positioned, so it paints over the fill a phone draws under the file,
      // which is positioned too, and under the marks that come after it.
      className={cn('pointer-events-none relative block text-accent', className)}
    />
  );
});

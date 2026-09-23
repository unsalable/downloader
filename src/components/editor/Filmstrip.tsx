import { memo, useCallback, useEffect, useRef } from 'react';

import { useTranslation } from '@/i18n';
import { cn } from '@/lib/cn';
import type { FilmstripData } from '@/types';

/**
 * The video track of the editor's timeline: one canvas tiling the thumbnails
 * FFmpeg rendered on the Rust side.
 *
 * It is deliberately not a row of <img> elements. The strip is redrawn on
 * every frame of a scrub or a zoom, and laying out a few hundred elements to
 * do it would put the work on the main thread's slowest path; one canvas
 * redraws in a single pass and keeps the DOM at one node. It is deliberately
 * not driven by the <video> element either -- the picture element cannot be
 * asked for a frame it is not showing, and a file the window cannot decode
 * still has to get a usable timeline.
 *
 * It is not interactive and holds nothing but the decoded chunks it draws; the
 * surface above it owns the pointer, the playhead and the marks.
 *
 * It also reads no colour from the theme, which is why -- unlike the waveform
 * beside it -- it watches nothing. The canvas is transparent where no chunk
 * has arrived, and what shows through is the track's own fill, which CSS keeps
 * in step with the theme without anything being painted again. The track's
 * rather than the canvas's: on a phone the file does not reach the ends of the
 * track, and a filled canvas would paint the gutters beside it as film.
 *
 * Memoised: its props change when the view does, and never while the video
 * merely plays.
 */

interface FilmstripProps {
  data: FilmstripData | null;
  /** The left edge of what is visible, in seconds from the start of the file. */
  windowStartSec: number;
  /** The right edge, in the same units. */
  windowEndSec: number;
  /** The visible width of the track in CSS pixels, which the parent measures. */
  width: number;
  height: number;
  /** More chunks are still coming, so what is drawn is the strip so far. */
  loading?: boolean;
  className?: string;
}

/**
 * What has been decoded, and what is on its way.
 *
 * Chunks arrive one at a time so the strip fills from the left, which means
 * the same `data` object is handed back repeatedly with one more entry. The
 * two strings identify the strip those entries belong to: the geometry alone
 * would match the same file reopened at the same zoom, and the first chunk
 * never changes while the rest of them land.
 */
interface StripCache {
  geometry: string;
  head: string;
  decoded: Map<number, HTMLImageElement>;
  pending: Map<number, HTMLImageElement>;
}

function geometryOf(data: FilmstripData): string {
  return [
    data.frames,
    data.chunkFrames,
    data.cellWidth,
    data.cellHeight,
    data.startSec,
    data.lengthSec,
  ].join('/');
}

/** Let go of every chunk that has not finished, so a decode cannot land on a dead component. */
function abandon(cache: StripCache) {
  for (const image of cache.pending.values()) {
    image.onload = null;
    image.onerror = null;
  }
  cache.pending.clear();
}

export const Filmstrip = memo(function Filmstrip({
  data,
  windowStartSec,
  windowEndSec,
  width,
  height,
  loading = false,
  className,
}: FilmstripProps) {
  const { t } = useTranslation();
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const frame = useRef<number | null>(null);
  const cache = useRef<StripCache>({
    geometry: '',
    head: '',
    decoded: new Map(),
    pending: new Map(),
  });

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

    // The backing store is in device pixels while the geometry below is in CSS
    // pixels. The backend renders cells in device pixels too, so at the usual
    // zoom a cell lands one source pixel to one screen pixel through this.
    ctx.setTransform(ratio, 0, 0, ratio, 0, 0);
    // Cleared rather than filled: what shows through is the track's own
    // fill, which is also what stands in for the chunks still coming.
    ctx.clearRect(0, 0, width, height);

    if (!data || data.frames <= 0 || data.chunkFrames <= 0) return;
    if (data.cellHeight <= 0 || data.cellWidth <= 0 || data.lengthSec <= 0) return;

    const span = windowEndSec - windowStartSec;
    if (span <= 0) return;

    // A cell is drawn at the shape it was rendered at and the strip steps
    // across by that width. Stretching one cell to fill whatever band the
    // window happens to give it would distort the picture, which on a
    // filmstrip is the only thing the user is reading.
    const scale = height / data.cellHeight;
    const cellWidth = data.cellWidth * scale;
    if (cellWidth <= 0) return;

    const dataEnd = data.startSec + data.lengthSec;
    const perSecond = data.frames / data.lengthSec;

    // Where the strip's own stretch of the file lies on the canvas. The cells
    // are tiled from its start rather than from the canvas's edge, and cut off
    // at its end: on a phone the file is laid out with a gutter at either end
    // of the track, and a cell tiled from the edge would hang out into it.
    // Wherever the file runs off the side -- all of the desktop's track at
    // every scale -- this is the canvas's own edge, and nothing changes.
    const fromX = Math.max(0, ((data.startSec - windowStartSec) / span) * width);
    const toX = Math.min(width, ((dataEnd - windowStartSec) / span) * width);
    if (toX <= fromX) return;
    ctx.save();
    ctx.beginPath();
    ctx.rect(fromX, 0, toX - fromX, height);
    ctx.clip();

    for (let x = fromX; x < toX; x += cellWidth) {
      // The middle of the band is what picks the frame, so the cell shown is
      // the one nearest to the time the band is mostly about. Every band here
      // starts inside the strip, but the last one can have its middle past the
      // strip's end: it shows the last frame, cut off by the clip, rather than
      // leaving the end of the file bare -- on a phone's track, where a cell
      // spans seconds, that bare stretch was a tenth of a short clip.
      const seconds = Math.min(windowStartSec + ((x + cellWidth / 2) / width) * span, dataEnd);
      // A window wider than the strip has bands no frame answers for, and the
      // bare surface is the honest drawing of that.
      if (seconds < data.startSec) continue;

      // Cell k is the frame half a step into its own slice, which is how the
      // backend chose it; the half step is what makes this the nearest cell
      // rather than the one before it. At the strip's very end that rounds up
      // to a cell one past the last, which is the last.
      const index = Math.min(Math.round((seconds - data.startSec) * perSecond - 0.5), data.frames - 1);
      if (index < 0) continue;

      const chunk = Math.floor(index / data.chunkFrames);
      const cell = index - chunk * data.chunkFrames;
      const image = cache.current.decoded.get(chunk);
      if (!image) continue;
      // The last chunk holds whatever was left rather than a full set, so a
      // cell past the end of the sprite is one that was never rendered.
      if ((cell + 1) * data.cellWidth > image.naturalWidth) continue;

      ctx.drawImage(
        image,
        cell * data.cellWidth,
        0,
        data.cellWidth,
        data.cellHeight,
        x,
        0,
        cellWidth,
        height,
      );
    }
    ctx.restore();
  }, [data, height, width, windowEndSec, windowStartSec]);

  const schedule = useCallback(() => {
    if (frame.current != null) return;
    // Several things can invalidate the strip within one frame -- a scroll
    // moves the window while a chunk finishes decoding -- and each of them
    // paints the same picture.
    frame.current = requestAnimationFrame(() => {
      frame.current = null;
      paint();
    });
  }, [paint]);

  useEffect(() => {
    const current = cache.current;

    if (!data) {
      // The clip was closed or swapped out. Holding its sprites would keep a
      // strip's worth of decoded bitmaps alive for a file nobody is editing.
      abandon(current);
      current.decoded.clear();
      current.geometry = '';
      current.head = '';
      return;
    }

    const geometry = geometryOf(data);
    const head = data.chunks[0] ?? '';
    if (geometry !== current.geometry || head !== current.head) {
      abandon(current);
      current.decoded.clear();
      current.geometry = geometry;
      current.head = head;
    }

    data.chunks.forEach((chunk, index) => {
      if (!chunk || current.decoded.has(index) || current.pending.has(index)) return;
      const image = new Image();
      current.pending.set(index, image);
      image.onload = () => {
        current.pending.delete(index);
        current.decoded.set(index, image);
        schedule();
      };
      image.onerror = () => {
        // A chunk that will not decode simply never appears. The strip is an
        // aid to finding a moment, not the thing being edited, so a gap in it
        // is not worth interrupting anyone over.
        current.pending.delete(index);
      };
      image.src = chunk;
    });
  }, [data, schedule]);

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
    const current = cache.current;
    return () => {
      abandon(current);
      current.decoded.clear();
    };
  }, []);

  return (
    <canvas
      ref={canvasRef}
      role="img"
      aria-label={t('editor.videoTrack')}
      aria-busy={loading}
      style={{ width, height }}
      // Transparent to the pointer: the track underneath is what a press on the
      // timeline means, and a canvas swallowing it would be a dead strip.
      // Positioned, so it paints over the fill a phone draws under the file,
      // which is positioned too, and under the marks that come after it.
      className={cn('pointer-events-none relative block', className)}
    />
  );
});

import { useCallback, useRef } from 'react';

import { useTranslation } from '@/i18n';
import { cn } from '@/lib/cn';
import { formatTimecode } from '@/lib/format';

/**
 * How far one arrow key moves a mark, and how far with Shift held. A tenth is
 * the smallest step the readout can show; a second is the step for crossing a
 * clip rather than tuning an edge.
 */
const STEP_SEC = 0.1;
const COARSE_STEP_SEC = 1;

interface TimelineProps {
  durationSec: number;
  startSec: number;
  endSec: number;
  /** Where the video is, so the handles have something to be placed against. */
  positionSec: number;
  disabled?: boolean;
  onChangeStart: (seconds: number) => void;
  onChangeEnd: (seconds: number) => void;
  onSeek: (seconds: number) => void;
}

type Grip = 'start' | 'end' | 'playhead';

/**
 * The range scrubber: one track carrying both marks and the playhead.
 *
 * Two `Slider`s stacked would have been less code and the wrong object -- they
 * would each own their own track, so the region between the marks (which is
 * the whole point) would have nothing to live on, and either handle could be
 * dragged through the other. This owns one track and treats the pair as one
 * value, which is also what lets a handle stop against its partner rather than
 * past it.
 *
 * Pointer capture rather than window listeners: the drag keeps receiving moves
 * once the pointer leaves the element, and the browser ends it for us if the
 * window loses focus mid-drag.
 */
export function Timeline({
  durationSec,
  startSec,
  endSec,
  positionSec,
  disabled = false,
  onChangeStart,
  onChangeEnd,
  onSeek,
}: TimelineProps) {
  const { t } = useTranslation();
  const trackRef = useRef<HTMLDivElement>(null);
  const grip = useRef<Grip | null>(null);

  const usable = durationSec > 0 ? durationSec : 1;
  const percent = (seconds: number) => Math.min(100, Math.max(0, (seconds / usable) * 100));

  /** Where along the file a pointer is, in seconds. */
  const secondsAt = useCallback(
    (clientX: number): number => {
      const track = trackRef.current;
      if (!track) return 0;
      const rect = track.getBoundingClientRect();
      if (rect.width <= 0) return 0;
      const fraction = (clientX - rect.left) / rect.width;
      return Math.min(Math.max(fraction, 0), 1) * durationSec;
    },
    [durationSec],
  );

  const applyGrip = useCallback(
    (which: Grip, seconds: number) => {
      if (which === 'playhead') {
        onSeek(seconds);
        return;
      }
      // A mark stops against its partner instead of pushing or passing it; the
      // store does the stopping, because only it knows where the partner is by
      // the time this lands.
      if (which === 'start') onChangeStart(seconds);
      else onChangeEnd(seconds);
      // The frame under the handle is what the mark means, so the picture
      // follows it while it moves.
      onSeek(seconds);
    },
    [onChangeEnd, onChangeStart, onSeek],
  );

  const beginDrag = useCallback(
    (which: Grip) => (event: React.PointerEvent) => {
      if (disabled) return;
      event.preventDefault();
      // Capture is what keeps the drag alive once the pointer leaves the
      // track. It is refused for a pointer that is no longer down, which is
      // not a reason to drop the press: without it the drag still works, it
      // just ends at the edge.
      try {
        event.currentTarget.setPointerCapture(event.pointerId);
      } catch {
        // Not capturable. The move handler below is still wired up.
      }
      grip.current = which;
      applyGrip(which, secondsAt(event.clientX));
    },
    [applyGrip, disabled, secondsAt],
  );

  const onPointerMove = useCallback(
    (event: React.PointerEvent) => {
      if (!grip.current) return;
      applyGrip(grip.current, secondsAt(event.clientX));
    },
    [applyGrip, secondsAt],
  );

  const endDrag = useCallback((event: React.PointerEvent) => {
    grip.current = null;
    try {
      if (event.currentTarget.hasPointerCapture(event.pointerId)) {
        event.currentTarget.releasePointerCapture(event.pointerId);
      }
    } catch {
      // Already released, along with the drag it belonged to.
    }
  }, []);

  const onHandleKey = useCallback(
    (which: 'start' | 'end') => (event: React.KeyboardEvent) => {
      if (disabled) return;
      const step = event.shiftKey ? COARSE_STEP_SEC : STEP_SEC;
      const delta =
        event.key === 'ArrowLeft' ? -step : event.key === 'ArrowRight' ? step : null;

      if (delta == null) {
        // Home and End send a mark to the edge of the file it belongs to.
        if (event.key === 'Home' && which === 'start') applyGrip('start', 0);
        else if (event.key === 'End' && which === 'end') applyGrip('end', durationSec);
        else return;
        event.preventDefault();
        return;
      }
      event.preventDefault();
      applyGrip(which, (which === 'start' ? startSec : endSec) + delta);
    },
    [applyGrip, disabled, durationSec, endSec, startSec],
  );

  return (
    <div className={cn('select-none', disabled && 'pointer-events-none opacity-50')}>
      <div
        ref={trackRef}
        onPointerDown={beginDrag('playhead')}
        onPointerMove={onPointerMove}
        onPointerUp={endDrag}
        onPointerCancel={endDrag}
        className="relative h-12 cursor-pointer touch-none overflow-hidden rounded-[var(--radius-card)] bg-surface-sunken"
      >
        {/* The kept range is left alone and what falls outside it is dimmed,
            rather than the other way round: a whole file with nothing trimmed
            off it should look like a plain track, not like a filled one. */}
        <div
          className="pointer-events-none absolute inset-y-0 left-0 bg-bg/60"
          style={{ width: `${percent(startSec)}%` }}
        />
        <div
          className="pointer-events-none absolute inset-y-0 right-0 bg-bg/60"
          style={{ width: `${100 - percent(endSec)}%` }}
        />

        {/* The playhead. Drawn after the dimming so it stays visible on the
            part of the file that is being cut away. */}
        <div
          aria-hidden="true"
          className="pointer-events-none absolute inset-y-1 w-0.5 -translate-x-1/2 rounded-full bg-fg shadow-[0_0_0_1px_rgb(0_0_0/0.25)]"
          style={{ left: `${percent(positionSec)}%` }}
        />

        <Handle
          side="start"
          label={t('trim.startMark')}
          seconds={startSec}
          max={durationSec}
          left={percent(startSec)}
          onPointerDown={beginDrag('start')}
          onPointerMove={onPointerMove}
          onPointerUp={endDrag}
          onPointerCancel={endDrag}
          onKeyDown={onHandleKey('start')}
        />
        <Handle
          side="end"
          label={t('trim.endMark')}
          seconds={endSec}
          max={durationSec}
          left={percent(endSec)}
          onPointerDown={beginDrag('end')}
          onPointerMove={onPointerMove}
          onPointerUp={endDrag}
          onPointerCancel={endDrag}
          onKeyDown={onHandleKey('end')}
        />
      </div>

      {/* The two ends of the file, so the track has a scale. */}
      <div className="mt-1.5 flex justify-between px-0.5">
        <span className="tabular text-[11.5px] text-fg-faint">{formatTimecode(0)}</span>
        <span className="tabular text-[11.5px] text-fg-faint">{formatTimecode(durationSec)}</span>
      </div>
    </div>
  );
}

interface HandleProps {
  side: 'start' | 'end';
  label: string;
  seconds: number;
  max: number;
  left: number;
  onPointerDown: (event: React.PointerEvent) => void;
  onPointerMove: (event: React.PointerEvent) => void;
  onPointerUp: (event: React.PointerEvent) => void;
  onPointerCancel: (event: React.PointerEvent) => void;
  onKeyDown: (event: React.KeyboardEvent) => void;
}

/**
 * One mark. The grab area is wider than the bar it draws, because a 3px target
 * is a 3px target however good it looks.
 */
function Handle({ side, label, seconds, max, left, ...handlers }: HandleProps) {
  return (
    <div
      role="slider"
      tabIndex={0}
      aria-label={label}
      aria-valuemin={0}
      aria-valuemax={Math.round(max * 10) / 10}
      aria-valuenow={Math.round(seconds * 10) / 10}
      aria-valuetext={formatTimecode(seconds)}
      {...handlers}
      className={cn(
        'absolute inset-y-0 flex w-5 cursor-ew-resize touch-none items-center justify-center',
        'outline-none focus-visible:ring-2 focus-visible:ring-accent focus-visible:ring-offset-0',
        'rounded-[3px]',
        side === 'start' ? '-translate-x-1/2' : '-translate-x-1/2',
      )}
      style={{ left: `${left}%` }}
    >
      <span
        aria-hidden="true"
        className={cn(
          'h-8 w-[5px] rounded-full bg-accent',
          'shadow-[0_0_0_0.5px_rgb(0_0_0/0.12),0_1px_3px_rgb(0_0_0/0.25)]',
        )}
      />
    </div>
  );
}

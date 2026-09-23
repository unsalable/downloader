import { AnimatePresence, motion } from 'motion/react';
import { useCallback, useEffect, useRef, useState } from 'react';

import { Button } from '@/components/ui/Button';
import { Dropdown, type DropdownOption } from '@/components/ui/Dropdown';
import { InlineNotice } from '@/components/ui/InlineNotice';
import { Modal } from '@/components/ui/Modal';
import { PlatformBadge } from '@/components/ui/PlatformBadge';
import { Progress } from '@/components/ui/Progress';
import { TextInput } from '@/components/ui/TextInput';
import { errorMessage, useTranslation } from '@/i18n';
import { cn } from '@/lib/cn';
import { formatBytes, formatDuration, formatTimecode } from '@/lib/format';
import { COLLAPSE } from '@/lib/motion';
import * as ipc from '@/services/ipc';
import { useEditorStore } from '@/stores/useEditorStore';
import type { MediaMetadata } from '@/types';

/**
 * Bringing a link into the editor: paste, look, and take either a slice of it
 * or all of it.
 *
 * It is deliberately not a second download screen. There is no format picker,
 * no thumbnail and no queue -- the file it produces is on its way to the
 * timeline, where every one of those decisions is made again and better. The
 * range it offers is coarse on purpose: nothing has been downloaded yet, so
 * there is no filmstrip and no waveform to cut against, and the fetch lands on
 * the nearest keyframe anyway.
 */

/**
 * The shortest range worth fetching, and how far an arrow key moves a handle.
 *
 * A second per press rather than the tenth the editor's own timeline uses:
 * this picker is placing a rough window over a whole video, and a tenth of a
 * second would be a hundred presses to cross a minute.
 */
const MIN_SPAN_SEC = 1;
const STEP_SEC = 1;
const COARSE_STEP_SEC = 10;

/**
 * What the fetch settles for, in pixels of height.
 *
 * It defaults to 1080 rather than to the source, and that is a deliberate
 * opinion: a 4K link fetched whole is well over a gigabyte, and this file is on
 * its way to a screen that will re-encode it into something far smaller. Anyone
 * who does want every pixel says so here, once, before the bytes move.
 */
const HEIGHT_LADDER = [2160, 1440, 1080, 720, 480] as const;
const DEFAULT_HEIGHT = 1080;

interface LinkImportModalProps {
  open: boolean;
  onClose: () => void;
  /** The finished file, once. The caller opens it as a clip. */
  onFetched: (path: string) => void;
}

interface Range {
  start: number;
  end: number;
}

function clamp(value: number, low: number, high: number): number {
  return Math.min(Math.max(value, low), high);
}

export function LinkImportModal({ open, onClose, onFetched }: LinkImportModalProps) {
  const { t } = useTranslation();

  const fetchState = useEditorStore((state) => state.fetch);
  const cancelFetch = useEditorStore((state) => state.cancelFetch);

  const [url, setUrl] = useState('');
  const [checking, setChecking] = useState(false);
  const [checkError, setCheckError] = useState<string | null>(null);
  const [metadata, setMetadata] = useState<MediaMetadata | null>(null);
  const [range, setRange] = useState<Range | null>(null);

  // A fetch that this sheet started, as opposed to one already sitting in the
  // store from an earlier visit. Without it, reopening the sheet after a
  // finished fetch would hand the caller that old file again.
  const [fetching, setFetching] = useState(false);
  const [fetchError, setFetchError] = useState<string | null>(null);
  const [maxHeight, setMaxHeight] = useState<number | null>(DEFAULT_HEIGHT);

  const duration = metadata?.durationSec ?? 0;
  const hasRangePicker = (metadata?.rangeFetchable ?? false) && duration > MIN_SPAN_SEC;
  // Only a window narrower than the video is a range; leaving the handles
  // where they started means the user asked for all of it.
  const chosen =
    hasRangePicker && range != null && (range.start > 0 || range.end < duration);

  // The sheet keeps nothing between visits. A link checked yesterday may not
  // resolve the same way today, so the next open starts from an empty field.
  useEffect(() => {
    if (open) return;
    setUrl('');
    setChecking(false);
    setCheckError(null);
    setMetadata(null);
    setRange(null);
    setFetching(false);
    setFetchError(null);
    setMaxHeight(DEFAULT_HEIGHT);
  }, [open]);

  useEffect(() => {
    if (!fetching) return;
    if (fetchState.status === 'completed') {
      const path = fetchState.outputPath;
      setFetching(false);
      if (path) {
        onFetched(path);
        onClose();
      }
      return;
    }
    if (fetchState.status === 'failed') {
      setFetching(false);
      setFetchError(fetchState.error ? errorMessage(fetchState.error) : null);
      return;
    }
    if (fetchState.status === 'canceled') {
      // The user stopped it themselves, which needs no explaining back to them.
      setFetching(false);
    }
  }, [fetchState, fetching, onClose, onFetched]);

  const check = useCallback(async () => {
    const target = url.trim();
    if (target.length === 0 || checking) return;
    setChecking(true);
    setCheckError(null);
    try {
      const found = await ipc.analyzeUrl(target);
      setMetadata(found);
      const length = found.durationSec ?? 0;
      setRange(length > MIN_SPAN_SEC ? { start: 0, end: length } : null);
    } catch (caught) {
      setMetadata(null);
      setRange(null);
      setCheckError(errorMessage(ipc.toAppError(caught)));
    } finally {
      setChecking(false);
    }
  }, [checking, url]);

  const startFetch = useCallback(async () => {
    if (!metadata || fetching) return;
    setFetchError(null);
    setFetching(true);
    try {
      await ipc.startRangeFetch({
        url: url.trim(),
        startSec: chosen && range ? range.start : null,
        endSec: chosen && range ? range.end : null,
        maxHeight,
        // The editor re-cuts this file frame by frame, so paying twice the
        // time for an exact cut here would buy something it throws away.
        exact: false,
        outputDir: null,
      });
    } catch (caught) {
      setFetching(false);
      setFetchError(errorMessage(ipc.toAppError(caught)));
    }
  }, [chosen, fetching, maxHeight, metadata, range, url]);

  return (
    <Modal
      open={open}
      onClose={onClose}
      width={520}
      title={t('editor.linkTitle')}
      closeLabel={t('common.close')}
      footer={
        fetching ? (
          <Button variant="ghost" onClick={() => void cancelFetch()}>
            {t('common.cancel')}
          </Button>
        ) : (
          <>
            <Button variant="ghost" onClick={onClose}>
              {t('common.cancel')}
            </Button>
            <Button
              variant="primary"
              disabled={metadata == null}
              onClick={() => void startFetch()}
            >
              {chosen ? t('editor.fetchRange') : t('editor.fetchWhole')}
            </Button>
          </>
        )
      }
    >
      <div className="pb-3">
        <div className="flex items-end gap-2">
          <div className="min-w-0 flex-1">
            <TextInput
              data-autofocus
              value={url}
              spellCheck={false}
              autoComplete="off"
              placeholder={t('editor.linkPlaceholder')}
              disabled={fetching}
              onChange={(event) => {
                setUrl(event.target.value);
                // What was found belongs to the link that was checked. Once
                // the field says something else, so does the sheet.
                setMetadata(null);
                setRange(null);
                setCheckError(null);
                setFetchError(null);
              }}
              onKeyDown={(event) => {
                if (event.key !== 'Enter') return;
                event.preventDefault();
                void check();
              }}
            />
          </div>
          <Button
            variant="secondary"
            loading={checking}
            disabled={url.trim().length === 0 || fetching}
            onClick={() => void check()}
          >
            {t('editor.linkCheck')}
          </Button>
        </div>

        {checkError && (
          <InlineNotice tone="error" className="mt-2">
            {checkError}
          </InlineNotice>
        )}

        <AnimatePresence initial={false}>
          {metadata && (
            <motion.div
              variants={COLLAPSE}
              initial="initial"
              animate="animate"
              exit="exit"
              className="overflow-hidden"
            >
              <div className="mt-4 flex items-center gap-3">
                <PlatformBadge platform={metadata.platform} size="lg" />
                <div className="min-w-0 flex-1">
                  <p className="truncate text-[13.5px] font-medium text-fg">{metadata.title}</p>
                  <p className="mt-0.5 flex gap-x-3 text-[12.5px] leading-[18px] text-fg-muted">
                    <span className="truncate">{metadata.platformLabel}</span>
                    {metadata.durationSec != null && (
                      <span className="tabular shrink-0">
                        {formatDuration(metadata.durationSec)}
                      </span>
                    )}
                  </p>
                </div>
              </div>

              {/* Which links can hand over a slice of themselves is decided by
                  the backend, from the protocol of the streams it would really
                  fetch. Naming a site here, or keeping a list of who supports
                  what, would be the same fact written down twice -- and the
                  copy would start lying the day a site changed how it
                  delivers, with nobody here any the wiser. */}
              <p className="mt-3 text-[12.5px] leading-relaxed text-fg-muted">
                {metadata.rangeFetchable ? t('editor.rangeSupported') : t('editor.rangeWhole')}
              </p>

              <div className="mt-3 flex items-center justify-between gap-3">
                <span className="text-[12.5px] text-fg-muted">{t('editor.resolution')}</span>
                <Dropdown
                  value={maxHeight == null ? 'source' : String(maxHeight)}
                  onChange={(value) => setMaxHeight(value === 'source' ? null : Number(value))}
                  disabled={fetching}
                  className="w-[150px]"
                  options={[
                    { value: 'source', label: t('editor.sourceSame') },
                    ...HEIGHT_LADDER.map((height): DropdownOption<string> => ({
                      value: String(height),
                      label: `${height}p`,
                    })),
                  ]}
                />
              </div>

              {hasRangePicker && range && (
                <div className="mt-3">
                  <RangeBar
                    duration={duration}
                    range={range}
                    disabled={fetching}
                    onChange={setRange}
                    startLabel={t('editor.startMark')}
                    endLabel={t('editor.endMark')}
                  />
                  <div className="mt-2 flex items-center justify-between text-[12px] text-fg-faint">
                    <span className="tabular">{formatTimecode(range.start)}</span>
                    <span className="tabular">
                      {t('editor.length')} {formatTimecode(range.end - range.start)}
                    </span>
                    <span className="tabular">{formatTimecode(range.end)}</span>
                  </div>
                  <p className="mt-2 text-[12.5px] leading-relaxed text-fg-muted">
                    {t('editor.rangeCoarse')}
                  </p>
                </div>
              )}

              {fetching && (
                <div className="mt-4 rounded-[var(--radius-control)] bg-surface-sunken p-3">
                  {/* The backend puts the whole-file fallback in the title:
                      a range was asked for and the chosen streams could not
                      give one, so it says so here rather than quietly handing
                      back more than was wanted. */}
                  {fetchState.title && (
                    <p className="mb-1.5 truncate text-[12.5px] text-fg-muted">
                      {fetchState.title}
                    </p>
                  )}
                  <div className="tabular mb-2 flex items-center justify-between text-[12.5px] text-fg-muted">
                    <span>
                      {fetchState.status === 'fetching'
                        ? t('editor.fetching')
                        : t('editor.resolving')}
                    </span>
                    {fetchState.receivedBytes > 0 && (
                      <span>{formatBytes(fetchState.receivedBytes)}</span>
                    )}
                  </div>
                  {/* Null percent is not zero: a ranged fetch through the
                      engine says nothing until it is over, and a bar resting
                      at 0 for a minute claims progress that has not happened. */}
                  <Progress value={fetchState.percent} label={t('editor.fetching')} />
                </div>
              )}

              {fetchError && (
                <InlineNotice tone="error" className="mt-2">
                  {fetchError}
                </InlineNotice>
              )}
            </motion.div>
          )}
        </AnimatePresence>
      </div>
    </Modal>
  );
}

interface RangeBarProps {
  duration: number;
  range: Range;
  disabled: boolean;
  onChange: (range: Range) => void;
  startLabel: string;
  endLabel: string;
}

/**
 * Two marks over the length of the video, and nothing else.
 *
 * One track carrying both, rather than two sliders: the span between the marks
 * is the whole point, and it needs something to be drawn on. Pointer capture
 * rather than window listeners, so a drag survives the pointer leaving the bar
 * and the browser ends it if the window loses focus.
 */
function RangeBar({
  duration,
  range,
  disabled,
  onChange,
  startLabel,
  endLabel,
}: RangeBarProps) {
  const trackRef = useRef<HTMLDivElement>(null);
  const grip = useRef<'start' | 'end' | null>(null);
  // Read inside the pointer handlers, which are captured once per drag and
  // would otherwise be moving the handle from where it was when the press
  // began rather than from where it is now.
  const latest = useRef(range);
  latest.current = range;

  const percent = (seconds: number) => clamp((seconds / duration) * 100, 0, 100);

  const secondsAt = useCallback(
    (clientX: number): number => {
      const track = trackRef.current;
      if (!track) return 0;
      const rect = track.getBoundingClientRect();
      if (rect.width <= 0) return 0;
      return clamp((clientX - rect.left) / rect.width, 0, 1) * duration;
    },
    [duration],
  );

  const move = useCallback(
    (which: 'start' | 'end', seconds: number) => {
      const current = latest.current;
      if (which === 'start') {
        const start = clamp(seconds, 0, current.end - MIN_SPAN_SEC);
        if (start !== current.start) onChange({ ...current, start });
        return;
      }
      const end = clamp(seconds, current.start + MIN_SPAN_SEC, duration);
      if (end !== current.end) onChange({ ...current, end });
    },
    [duration, onChange],
  );

  const beginDrag = useCallback(
    (which: 'start' | 'end') => (event: React.PointerEvent) => {
      if (disabled) return;
      event.preventDefault();
      // The press belongs to the handle and to nothing above it: the dialog
      // and the track both take presses of their own, and either one arriving
      // after this would end the drag before it started.
      event.stopPropagation();
      // Capture keeps the drag alive once the pointer leaves the bar. It is
      // refused for a pointer that is no longer down, which is not a reason to
      // drop the press -- without it the drag still works, it just ends at the
      // edge.
      try {
        event.currentTarget.setPointerCapture(event.pointerId);
      } catch {
        // Not capturable. The move handler below is still wired up.
      }
      grip.current = which;
      move(which, secondsAt(event.clientX));
    },
    [disabled, move, secondsAt],
  );

  const onPointerMove = useCallback(
    (event: React.PointerEvent) => {
      if (!grip.current) return;
      event.stopPropagation();
      move(grip.current, secondsAt(event.clientX));
    },
    [move, secondsAt],
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

  const onKeyDown = useCallback(
    (which: 'start' | 'end') => (event: React.KeyboardEvent) => {
      if (disabled) return;
      const step = event.shiftKey ? COARSE_STEP_SEC : STEP_SEC;
      const delta =
        event.key === 'ArrowLeft' ? -step : event.key === 'ArrowRight' ? step : null;
      if (delta == null) return;
      event.preventDefault();
      move(which, (which === 'start' ? latest.current.start : latest.current.end) + delta);
    },
    [disabled, move],
  );

  return (
    <div
      ref={trackRef}
      className={cn(
        'relative h-9 touch-none select-none overflow-hidden',
        'rounded-[var(--radius-control)] bg-surface-sunken',
        disabled && 'pointer-events-none opacity-50',
      )}
    >
      {/* What falls outside the range is dimmed rather than the range itself
          being filled: a window over the whole video should look like a plain
          bar, not like a full one. */}
      <div
        aria-hidden="true"
        className="pointer-events-none absolute inset-y-0 left-0 bg-bg/60"
        style={{ width: `${percent(range.start)}%` }}
      />
      <div
        aria-hidden="true"
        className="pointer-events-none absolute inset-y-0 right-0 bg-bg/60"
        style={{ width: `${100 - percent(range.end)}%` }}
      />

      <Handle
        label={startLabel}
        seconds={range.start}
        max={duration}
        left={percent(range.start)}
        onPointerDown={beginDrag('start')}
        onPointerMove={onPointerMove}
        onPointerUp={endDrag}
        onPointerCancel={endDrag}
        onKeyDown={onKeyDown('start')}
      />
      <Handle
        label={endLabel}
        seconds={range.end}
        max={duration}
        left={percent(range.end)}
        onPointerDown={beginDrag('end')}
        onPointerMove={onPointerMove}
        onPointerUp={endDrag}
        onPointerCancel={endDrag}
        onKeyDown={onKeyDown('end')}
      />
    </div>
  );
}

interface HandleProps {
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

/** One mark. The grab area is wider than the bar it draws, because a 5px
 *  target is a 5px target however good it looks. */
function Handle({ label, seconds, max, left, ...handlers }: HandleProps) {
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
        'absolute inset-y-0 flex w-5 -translate-x-1/2 cursor-ew-resize touch-none',
        'items-center justify-center rounded-[3px]',
        'outline-none focus-visible:ring-2 focus-visible:ring-accent focus-visible:ring-offset-0',
      )}
      style={{ left: `${left}%` }}
    >
      <span
        aria-hidden="true"
        className={cn(
          'h-6 w-[5px] rounded-full bg-accent',
          'shadow-[0_0_0_0.5px_rgb(0_0_0/0.12),0_1px_3px_rgb(0_0_0/0.25)]',
        )}
      />
    </div>
  );
}

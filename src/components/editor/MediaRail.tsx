import { AnimatePresence, motion } from 'motion/react';
import { Link2, Plus, X } from 'lucide-react';
import { memo, useEffect, useState } from 'react';

import { IconButton } from '@/components/ui/IconButton';
import { Spinner } from '@/components/ui/Spinner';
import { Tooltip } from '@/components/ui/Tooltip';
import { useTranslation } from '@/i18n';
import { cn } from '@/lib/cn';
import { rise } from '@/lib/motion';
import { formatDuration } from '@/lib/format';
import * as ipc from '@/services/ipc';

/*
 * The column of open clips, down the side of the editor.
 *
 * It is not a file list and not a panel: it never collapses and it has no
 * header, because the only way to add a clip lives at the bottom of it, and a
 * rail that can be hidden is a rail whose plus button can be hidden too. It
 * shows a frame and a length and nothing else -- the name is in the tooltip,
 * where it costs no width.
 *
 * On a phone it lies on its side, as a strip at the top of the Clip tab: a
 * screen that tall has no width to spare for a column, and a strip of clips
 * above the picture would take height from everything else for the sake of
 * something most edits never use. There the frames are larger, the name is
 * the accessible name alone, and the one remove button belongs to the clip
 * that is open -- a finger has no hover to reveal one per chip with.
 */

/**
 * The chip, and therefore the thumbnail, is 68px wide. The rail is that plus a
 * gutter on each side, which is what leaves room for the remove button to sit
 * over the corner of a frame without being clipped by the scroll container.
 */
const THUMB_WIDTH = 68;
const THUMB_HEIGHT = 38;

/** The phone's strip, where a frame has to be large enough to recognise. */
const STRIP_WIDTH = 96;
const STRIP_HEIGHT = 54;

/** Asked for at twice the drawn height, so it holds up on a 2x display. */
const FRAME_HEIGHT = 76;

/**
 * A tenth of the way in. The first frame of a clip is very often black or a
 * title card, and a tenth is far enough past it to show the actual picture
 * while still being the same part of the clip every time it is asked for.
 */
const FRAME_FRACTION = 0.1;

/**
 * Frames are drawn by Rust and handed over as data URIs. The map spares a
 * round trip for a clip that has already been drawn this session -- switching
 * clips remounts these chips -- and the in-flight map means two mounts in the
 * same frame ask once between them rather than once each.
 */
const memory = new Map<string, string>();
const inflight = new Map<string, Promise<string>>();

/** Bounded so a long session of opening and closing clips cannot grow it. */
const MAX_ENTRIES = 120;

function remember(key: string, dataUrl: string) {
  if (memory.size >= MAX_ENTRIES) {
    const oldest = memory.keys().next().value;
    if (oldest !== undefined) memory.delete(oldest);
  }
  memory.set(key, dataUrl);
}

/**
 * One frame for one clip, or null.
 *
 * Null covers both "not drawn yet" and "could not be drawn", because the chip
 * does the same thing in either case: shows its own fill. A clip whose frame
 * cannot be read is still perfectly editable, and an error marker here would
 * be a complaint about something the user cannot act on.
 */
function useClipFrame(path: string, durationSec: number): string | null {
  const key = `${path}|${FRAME_HEIGHT}|${(durationSec * FRAME_FRACTION).toFixed(2)}`;
  const [src, setSrc] = useState<string | null>(() => memory.get(key) ?? null);

  useEffect(() => {
    const cached = memory.get(key);
    if (cached) {
      setSrc(cached);
      return;
    }
    setSrc(null);

    let active = true;
    let request = inflight.get(key);
    if (!request) {
      request = ipc.frameAt(path, durationSec * FRAME_FRACTION, FRAME_HEIGHT);
      inflight.set(key, request);
      request.finally(() => inflight.delete(key)).catch(() => {});
    }

    request
      .then((dataUrl) => {
        remember(key, dataUrl);
        if (active) setSrc(dataUrl);
      })
      .catch(() => {
        // Nothing to show and nothing to say. See the note above.
      });

    return () => {
      active = false;
    };
  }, [durationSec, key, path]);

  return src;
}

/** Hoisted: a fresh variants object every render would restart the animation. */
const CLIP_RISE = rise(6);

export interface MediaRailClip {
  id: string;
  name: string;
  path: string;
  durationSec: number;
}

interface MediaRailProps {
  clips: MediaRailClip[];
  activeId: string | null;
  onActivate: (id: string) => void;
  onRemove: (id: string) => void;
  onAdd: () => void;
  onAddLink: () => void;
  /** A column down the side (the desktop), or a strip across (the phone). */
  orientation?: 'vertical' | 'horizontal';
  /** A file is being read in, and the add button says so. */
  adding?: boolean;
  className?: string;
}

/** Memoised: the page it sits on re-renders far more often than the clips change. */
export const MediaRail = memo(function MediaRail({
  clips,
  activeId,
  onActivate,
  onRemove,
  onAdd,
  onAddLink,
  orientation = 'vertical',
  adding = false,
  className,
}: MediaRailProps) {
  const { t } = useTranslation();

  if (orientation === 'horizontal') {
    return (
      <div
        aria-label={t('editor.media')}
        // The top padding is the room the open clip's remove button needs to
        // sit over the corner of its frame without the scroller clipping it.
        className={cn(
          'flex items-start gap-2.5 overflow-x-auto overscroll-x-contain pt-1.5',
          className,
        )}
      >
        <AnimatePresence initial={false}>
          {clips.map((clip) => (
            <StripChip
              key={clip.id}
              clip={clip}
              active={clip.id === activeId}
              // Removing the only clip is closing the editor, which the X at
              // the top already does.
              removable={clips.length > 1}
              onActivate={onActivate}
              onRemove={onRemove}
            />
          ))}
        </AnimatePresence>

        <button
          type="button"
          aria-label={t('editor.addMedia')}
          aria-busy={adding || undefined}
          disabled={adding}
          onClick={onAdd}
          className={cn(
            'pressable-sm flex shrink-0 items-center justify-center rounded-[var(--radius-thumb)]',
            'border border-dashed border-border-strong text-fg-muted active:bg-fill',
            'disabled:pointer-events-none',
          )}
          style={{ width: STRIP_WIDTH, height: STRIP_HEIGHT }}
        >
          {adding ? <Spinner size={18} /> : <Plus size={20} aria-hidden="true" />}
        </button>
        <button
          type="button"
          aria-label={t('editor.linkTitle')}
          onClick={onAddLink}
          className={cn(
            'pressable-sm flex shrink-0 items-center justify-center rounded-[var(--radius-thumb)]',
            'border border-dashed border-border-strong text-fg-muted active:bg-fill',
          )}
          style={{ width: STRIP_HEIGHT, height: STRIP_HEIGHT }}
        >
          <Link2 size={18} aria-hidden="true" />
        </button>
      </div>
    );
  }

  return (
    <div
      aria-label={t('editor.media')}
      className={cn(
        'flex w-[84px] shrink-0 flex-col items-center gap-2 overflow-y-auto overflow-x-hidden',
        'border-r border-[var(--border)] bg-surface-sunken py-3',
        className,
      )}
    >
      {/* `initial={false}` so opening the editor on a pool of clips draws them
          rather than playing them in. Only a clip that arrives afterwards is
          something the user did. */}
      <AnimatePresence initial={false}>
        {clips.map((clip) => (
          <ClipChip
            key={clip.id}
            clip={clip}
            active={clip.id === activeId}
            onActivate={onActivate}
            onRemove={onRemove}
          />
        ))}
      </AnimatePresence>

      <div className="flex shrink-0 flex-col items-center gap-1">
        <button
          type="button"
          aria-label={t('editor.addMedia')}
          onClick={onAdd}
          className={cn(
            'pressable-sm flex items-center justify-center rounded-[var(--radius-thumb)]',
            'border border-dashed border-border-strong text-fg-faint',
            'hover:border-accent hover:bg-accent-soft hover:text-accent',
          )}
          style={{ width: THUMB_WIDTH, height: THUMB_HEIGHT }}
        >
          <Plus size={17} aria-hidden="true" />
        </button>
        <IconButton
          icon={<Link2 size={15} />}
          label={t('editor.linkTitle')}
          size="sm"
          onClick={onAddLink}
        />
      </div>
    </div>
  );
});

interface ClipChipProps {
  clip: MediaRailClip;
  active: boolean;
  onActivate: (id: string) => void;
  onRemove: (id: string) => void;
}

/**
 * One clip. The frame and the length are the whole chip; at 68px a name would
 * be three truncated characters, so it is the tooltip and the accessible name
 * instead.
 */
const ClipChip = memo(function ClipChip({ clip, active, onActivate, onRemove }: ClipChipProps) {
  const { t } = useTranslation();
  const frame = useClipFrame(clip.path, clip.durationSec);

  return (
    <motion.div
      variants={CLIP_RISE}
      initial="initial"
      animate="animate"
      exit="exit"
      className="group relative shrink-0"
      style={{ width: THUMB_WIDTH }}
    >
      <Tooltip label={clip.name} side="right">
        <button
          type="button"
          aria-label={clip.name}
          aria-current={active || undefined}
          onClick={() => onActivate(clip.id)}
          className="block w-full text-center"
        >
          <span
            className={cn(
              'block overflow-hidden rounded-[var(--radius-thumb)] bg-surface-active',
              // The ring is drawn inside the chip's own width so a selected
              // clip does not nudge the rail's contents sideways.
              active && 'ring-2 ring-inset ring-accent',
            )}
            style={{ width: THUMB_WIDTH, height: THUMB_HEIGHT }}
          >
            {frame != null && (
              <img
                src={frame}
                alt=""
                className="no-drag h-full w-full object-cover"
              />
            )}
          </span>
          <span
            className={cn(
              'tabular mt-1 block text-[11.5px] leading-[14px]',
              active ? 'text-fg' : 'text-fg-muted',
            )}
          >
            {formatDuration(clip.durationSec)}
          </span>
        </button>
      </Tooltip>

      <IconButton
        icon={<X size={13} />}
        label={t('editor.removeClip')}
        size="sm"
        onClick={() => onRemove(clip.id)}
        // A permanent remove button on every chip is a row of small red
        // targets down the side of the screen; it earns its place only once
        // the pointer, or the keyboard, is actually on the clip.
        className={cn(
          'reveal-on-hover absolute right-0.5 top-0.5 bg-surface/90',
          'group-focus-within:opacity-100',
        )}
      />
    </motion.div>
  );
});

interface StripChipProps extends ClipChipProps {
  removable: boolean;
}

/** One clip on the phone's strip: a larger frame, and its length beneath. */
const StripChip = memo(function StripChip({
  clip,
  active,
  removable,
  onActivate,
  onRemove,
}: StripChipProps) {
  const { t } = useTranslation();
  const frame = useClipFrame(clip.path, clip.durationSec);

  return (
    <motion.div
      variants={CLIP_RISE}
      initial="initial"
      animate="animate"
      exit="exit"
      className="relative shrink-0"
      style={{ width: STRIP_WIDTH }}
    >
      <button
        type="button"
        aria-label={clip.name}
        aria-current={active || undefined}
        onClick={() => onActivate(clip.id)}
        className="pressable-sm block w-full text-center"
      >
        <span
          className="relative block overflow-hidden rounded-[var(--radius-thumb)] bg-surface-active"
          style={{ width: STRIP_WIDTH, height: STRIP_HEIGHT }}
        >
          {frame != null && (
            <img src={frame} alt="" className="no-drag h-full w-full object-cover" />
          )}
          {/* Over the frame rather than on its box, where the picture would
              cover it. */}
          {active && (
            <span
              aria-hidden="true"
              className="absolute inset-0 rounded-[var(--radius-thumb)] ring-2 ring-inset ring-accent"
            />
          )}
        </span>
        <span
          className={cn(
            'tabular mt-1 block text-[12px] leading-4',
            active ? 'text-fg' : 'text-fg-muted',
          )}
        >
          {formatDuration(clip.durationSec)}
        </span>
      </button>

      {active && removable && (
        // Drawn small on the corner of the frame and reached through a larger
        // square around it: the corner of a 54px frame is all the room there
        // is, and a fingertip is not that precise.
        <button
          type="button"
          aria-label={t('editor.removeClip')}
          onClick={() => onRemove(clip.id)}
          className="absolute -right-1.5 -top-1.5 flex size-9 items-center justify-center"
        >
          <span className="flex size-6 items-center justify-center rounded-full bg-surface text-fg shadow-soft">
            <X size={13} aria-hidden="true" />
          </span>
        </button>
      )}
    </motion.div>
  );
});

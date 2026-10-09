import { Check, ImageOff, Music, Play } from 'lucide-react';
import { useRef, type Dispatch, type RefObject, type SetStateAction } from 'react';

import { CHIP } from '@/components/home/MediaPreviewCard';
import { useNearViewport } from '@/hooks/useNearViewport';
import { useThumbnail } from '@/hooks/useThumbnail';
import { useTranslation } from '@/i18n';
import { cn } from '@/lib/cn';
import { formatDuration } from '@/lib/format';
import { galleryTitleKey, itemLabelKey } from '@/lib/gallerySelection';
import { IS_MOBILE } from '@/lib/platform';
import type { GalleryItem } from '@/types';

interface GalleryPickerProps {
  /** The items this download can take, in the post's order. */
  items: GalleryItem[];
  /** Positions that will be downloaded. May also hold positions not shown here
   *  (a photo, while only sound is asked for); they are kept for when it is. */
  picked: ReadonlySet<number>;
  onChange: Dispatch<SetStateAction<Set<number>>>;
}

/**
 * The items of a carousel or gallery as a grid of pictures, each one picked
 * or not, the way Photos picks from an album.
 *
 * Every tile starts ticked: the usual wish is the whole post, and pressing
 * Download straight away should still bring all of it. The grid is for the
 * times it is not -- only the second photo, or the video without the photos
 * around it. It scrolls inside its card, like the song list, so a post of
 * thirty-five photos does not push the download button off the screen.
 */
export function GalleryPicker({ items, picked, onChange }: GalleryPickerProps) {
  const { t } = useTranslation();
  const scroller = useRef<HTMLDivElement>(null);
  const title = t(galleryTitleKey(items));
  const all = items.every((item) => picked.has(item.position));

  // From the latest choice rather than the one this render saw, so two quick
  // presses both count.
  const toggle = (position: number) =>
    onChange((current) => {
      const next = new Set(current);
      if (next.has(position)) next.delete(position);
      else next.add(position);
      return next;
    });

  // Only the items shown change: a photo ticked while pictures were asked for
  // stays ticked through "sound only", so switching back finds it as it was.
  const toggleAll = () =>
    onChange((current) => {
      const everything = items.every((item) => current.has(item.position));
      const next = new Set(current);
      for (const item of items) {
        if (everything) next.delete(item.position);
        else next.add(item.position);
      }
      return next;
    });

  return (
    <section>
      <div className="mb-2 flex items-center justify-between px-4">
        <h2 className="text-[13px] font-semibold text-fg-muted">{title}</h2>
        <button
          type="button"
          onClick={toggleAll}
          className={cn(
            'pressable -mr-1.5 rounded-md px-1.5 text-[12.5px] font-medium text-accent',
            // A finger's worth of height on the phone, given back by the
            // margins so the heading row stays the height of its text.
            IS_MOBILE ? '-my-3 min-h-11' : 'py-0.5',
          )}
        >
          {all ? t('tracks.selectNone') : t('tracks.selectAll')}
        </button>
      </div>

      {/* The padding leaves room for a tile's focus ring, which the scrolling
          card would otherwise clip at its edges. */}
      <div
        ref={scroller}
        className="max-h-[360px] overflow-y-auto rounded-[var(--radius-card)] border border-card-edge bg-surface p-1.5"
      >
        <div
          role="group"
          aria-label={title}
          className={cn('grid gap-1.5', IS_MOBILE ? 'grid-cols-3' : 'grid-cols-4')}
        >
          {items.map((item) => (
            <GalleryTile
              key={item.position}
              item={item}
              on={picked.has(item.position)}
              onToggle={() => toggle(item.position)}
              scroller={scroller}
            />
          ))}
        </div>
      </div>
    </section>
  );
}

/**
 * One item: its picture, cut square, with a tick in the corner. The whole
 * tile is the control -- a hundred pixels across on a phone, well over a
 * fingertip -- and an item left out recedes rather than disappearing, so the
 * post still reads whole.
 */
function GalleryTile({
  item,
  on,
  onToggle,
  scroller,
}: {
  item: GalleryItem;
  on: boolean;
  onToggle: () => void;
  scroller: RefObject<HTMLDivElement | null>;
}) {
  const { t } = useTranslation();
  const tile = useRef<HTMLButtonElement>(null);
  const near = useNearViewport(tile, scroller);
  const { src, failed } = useThumbnail(near ? item.thumbnailUrl : null);
  // "No picture" only once there is certainly none. Not keyed off `loading`,
  // which reads false for the one render between the address arriving and the
  // fetch starting -- enough to flash the icon on every tile as it scrolls in.
  const blank = failed || !item.thumbnailUrl;

  return (
    <button
      ref={tile}
      type="button"
      role="checkbox"
      aria-checked={on}
      aria-label={t(itemLabelKey(item), { n: item.position })}
      onClick={onToggle}
      className="pressable relative aspect-square overflow-hidden rounded-[var(--radius-thumb)] bg-surface-sunken"
    >
      {/* Every layer is pinned to the tile's edges rather than given a share
          of its height, so the square holds whatever a browser does with the
          inside of a button. */}
      {!src && !blank && <div aria-hidden="true" className="skeleton absolute inset-0" />}

      {src && (
        <img
          src={src}
          alt=""
          aria-hidden="true"
          draggable={false}
          className={cn(
            'no-drag absolute inset-0 size-full object-cover transition-opacity duration-150',
            !on && 'opacity-45',
          )}
        />
      )}

      {blank && (
        <div
          aria-hidden="true"
          className={cn(
            'absolute inset-0 flex items-center justify-center text-fg-faint',
            !on && 'opacity-45',
          )}
        >
          {item.kind === 'audio' ? <Music size={20} /> : <ImageOff size={20} />}
        </div>
      )}

      {/* A photo needs no label; anything that plays says for how long, or
          at least that it plays. Held at the tick's height, so a video the
          source gave no length for -- Instagram's, as a rule -- does not shrink
          to a sliver of a pill beside it. */}
      {item.kind !== 'image' && (
        <span aria-hidden="true" className={cn(CHIP, 'tabular bottom-1.5 left-1.5 h-[22px]')}>
          {item.durationSec != null ? (
            formatDuration(item.durationSec)
          ) : item.kind === 'audio' ? (
            <Music size={11} />
          ) : (
            <Play size={11} fill="currentColor" />
          )}
        </span>
      )}

      {/* The open ring carries its own white edge and shadow, so it reads over
          a white photo as well as a dark one. */}
      <span
        aria-hidden="true"
        className={cn(
          'absolute bottom-1.5 right-1.5 flex size-[22px] items-center justify-center rounded-full transition-colors duration-150',
          on
            ? 'bg-accent text-accent-fg'
            : 'border-[1.5px] border-white/90 bg-black/10 shadow-[0_1px_2px_rgb(0_0_0/0.3)]',
        )}
      >
        {on && <Check size={13} strokeWidth={3} />}
      </span>
    </button>
  );
}

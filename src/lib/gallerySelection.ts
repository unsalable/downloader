import { translate, type TranslationKey } from '@/i18n';
import type { DownloadMode, GalleryItem } from '@/types';

/**
 * What picking from a carousel means: which of its items a download can take,
 * which are ticked, and what the button says about them.
 *
 * Kept apart from the grid so the rules can be tested without a page, and so
 * Home and the grid read the same answer -- the button counting one set of
 * pictures while the grid shows another is exactly the mismatch this avoids.
 */

/** What a set of items is, for the words around it. */
export type GalleryNoun = 'photo' | 'video' | 'mixed' | 'item';

/**
 * The items a download in this mode can take. "Sound only" takes nothing from
 * a photo -- picked there, the plan would fall back to saving the photo itself
 * (`plan::effective_mode`) -- so photos are not offered in it. A TikTok photo
 * post then offers nothing, and its soundtrack, which belongs to the post
 * rather than to any picture, is what downloads.
 */
export function pickableItems(items: readonly GalleryItem[], mode: DownloadMode): GalleryItem[] {
  return mode === 'audio' ? items.filter((item) => item.kind !== 'image') : [...items];
}

/** The items shown that are ticked. A tick on one not shown is kept, not counted. */
export function pickedOf(items: readonly GalleryItem[], picked: ReadonlySet<number>): GalleryItem[] {
  return items.filter((item) => picked.has(item.position));
}

/**
 * Positions to queue, in the post's order. Always named one by one, every item
 * included: a post that changed since it was shown is then refused rather
 * than queued with items no one saw.
 */
export function pickedPositions(items: readonly GalleryItem[], picked: ReadonlySet<number>): number[] {
  return pickedOf(items, picked).map((item) => item.position);
}

export function galleryNoun(items: readonly GalleryItem[]): GalleryNoun {
  const kinds = new Set(items.map((item) => item.kind));
  if (kinds.size === 1 && kinds.has('image')) return 'photo';
  if (kinds.size === 1 && kinds.has('video')) return 'video';
  if (kinds.size === 2 && kinds.has('image') && kinds.has('video')) return 'mixed';
  return 'item';
}

const TITLES: Record<GalleryNoun, TranslationKey> = {
  photo: 'gallery.photos',
  video: 'gallery.videos',
  mixed: 'gallery.mixed',
  item: 'gallery.items',
};

/** The heading over the grid: what the items it shows are. */
export const galleryTitleKey = (items: readonly GalleryItem[]): TranslationKey =>
  TITLES[galleryNoun(items)];

/** How a tile is read out: "2. fotoğraf". */
export function itemLabelKey(item: GalleryItem): TranslationKey {
  if (item.kind === 'image') return 'gallery.photoN';
  if (item.kind === 'video') return 'gallery.videoN';
  return 'gallery.itemN';
}

/**
 * The download button's words. Plain "İndir" when every item is ticked -- the
 * grid already shows that it is all of them -- and when none is, since the
 * button is off then anyway. A part of the post always says how many of what,
 * by what the ticked ones are: two photos of a mixed post are "2 fotoğraf".
 */
export function galleryDownloadLabel(
  items: readonly GalleryItem[],
  picked: ReadonlySet<number>,
): string {
  const chosen = pickedOf(items, picked);
  const n = chosen.length;
  if (n === 0 || n === items.length) return translate('action.download');
  switch (galleryNoun(chosen)) {
    case 'photo':
      return n === 1 ? translate('action.downloadPhoto') : translate('action.downloadPhotos', { n });
    case 'video':
      return n === 1 ? translate('action.downloadVideo') : translate('action.downloadVideos', { n });
    default:
      return n === 1 ? translate('action.downloadItem') : translate('action.downloadItems', { n });
  }
}

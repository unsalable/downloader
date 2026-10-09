import { afterAll, beforeAll, describe, expect, it, vi } from 'vitest';

import { setLanguage } from '@/i18n';
import type { GalleryItem, MediaKind } from '@/types';

import {
  galleryDownloadLabel,
  galleryNoun,
  itemLabelKey,
  pickableItems,
  pickedPositions,
} from './gallerySelection';

/** A post's items from their kinds, numbered as the post numbers them. */
const post = (...kinds: MediaKind[]): GalleryItem[] =>
  kinds.map((kind, index) => ({
    position: index + 1,
    kind,
    thumbnailUrl: `https://cdn.example.com/${index + 1}.jpg`,
    durationSec: kind === 'video' ? 14 : null,
  }));

const photos = (count: number) => post(...Array.from({ length: count }, () => 'image' as const));
const picked = (...positions: number[]) => new Set(positions);

describe('pickableItems', () => {
  it('offers everything for pictures', () => {
    const items = post('image', 'video', 'image');
    expect(pickableItems(items, 'image')).toEqual(items);
  });

  it('offers only what has sound when sound is all that is asked for', () => {
    const items = post('image', 'video', 'image', 'video');
    expect(pickableItems(items, 'audio').map((item) => item.position)).toEqual([2, 4]);
    expect(pickableItems(photos(3), 'audio')).toEqual([]);
  });
});

describe('galleryNoun', () => {
  it('names the set by what is in it', () => {
    expect(galleryNoun(photos(2))).toBe('photo');
    expect(galleryNoun(post('video', 'video'))).toBe('video');
    expect(galleryNoun(post('image', 'video'))).toBe('mixed');
    expect(galleryNoun(post('audio', 'audio'))).toBe('item');
    expect(galleryNoun(post('image', 'audio'))).toBe('item');
    expect(galleryNoun([])).toBe('item');
  });
});

describe('pickedPositions', () => {
  it('queues the ticked items in the post’s order', () => {
    expect(pickedPositions(photos(5), picked(4, 2))).toEqual([2, 4]);
  });

  it('ignores a tick on an item that is not shown', () => {
    expect(pickedPositions(photos(3), picked(2, 7))).toEqual([2]);
  });

  it('names every item when every item is ticked', () => {
    expect(pickedPositions(photos(5), picked(1, 2, 3, 4, 5))).toEqual([1, 2, 3, 4, 5]);
  });
});

describe('galleryDownloadLabel', () => {
  it('says plain Download for all of them, or for none', () => {
    expect(galleryDownloadLabel(photos(5), picked(1, 2, 3, 4, 5))).toBe('Download');
    expect(galleryDownloadLabel(photos(5), picked())).toBe('Download');
  });

  it('says how many of what for a part of the post', () => {
    expect(galleryDownloadLabel(photos(5), picked(2))).toBe('Download photo');
    expect(galleryDownloadLabel(photos(5), picked(1, 3, 5))).toBe('Download 3 photos');
    expect(galleryDownloadLabel(post('video', 'image', 'image'), picked(1, 2))).toBe(
      'Download 2 items',
    );
    expect(galleryDownloadLabel(post('video', 'image', 'image'), picked(2, 3))).toBe(
      'Download 2 photos',
    );
  });

  describe('in Turkish', () => {
    beforeAll(() => {
      vi.stubGlobal('document', { documentElement: { lang: 'en' } });
      setLanguage('tr');
    });
    afterAll(() => {
      setLanguage('en');
      vi.unstubAllGlobals();
    });

    it('reads as Turkish does', () => {
      expect(galleryDownloadLabel(photos(5), picked(1, 3, 5))).toBe('3 fotoğrafı indir');
      expect(galleryDownloadLabel(photos(5), picked(2))).toBe('Fotoğrafı indir');
      expect(galleryDownloadLabel(post('video', 'image', 'image'), picked(1, 2))).toBe(
        '2 öğeyi indir',
      );
      expect(galleryDownloadLabel(post('video', 'video', 'image'), picked(1, 2))).toBe(
        '2 videoyu indir',
      );
      expect(galleryDownloadLabel(photos(5), picked(1, 2, 3, 4, 5))).toBe('İndir');
    });
  });
});

describe('itemLabelKey', () => {
  it('reads a tile out by what it is', () => {
    const [photo, video, song] = post('image', 'video', 'audio');
    expect(itemLabelKey(photo!)).toBe('gallery.photoN');
    expect(itemLabelKey(video!)).toBe('gallery.videoN');
    expect(itemLabelKey(song!)).toBe('gallery.itemN');
  });
});

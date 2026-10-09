import { describe, expect, it } from 'vitest';

import type { Handoff } from '@/types';

import { homeFromHandoff, titledFromHandoff } from './handoff';

const DEFAULTS = {
  defaultMode: 'video' as const,
  defaultQuality: { type: 'maxHeight' as const, height: 1080 },
  defaultContainer: 'mp4',
};

const STREAM: Handoff = {
  url: 'https://cdn.example.com/hls/master.m3u8',
  kind: 'stream',
  title: 'Bölüm 5',
  pageUrl: 'https://dizi.example.com/izle/5',
  referer: 'https://player.example.com/',
  origin: 'https://player.example.com',
  userAgent: 'Mozilla/5.0',
  thumbnail: 'https://cdn.example.com/poster.jpg',
  receivedAt: 1_790_000_000,
};

describe('homeFromHandoff', () => {
  it('opens Home on the default options, with the page it played on', () => {
    expect(homeFromHandoff(STREAM, DEFAULTS)).toEqual({
      url: 'https://cdn.example.com/hls/master.m3u8',
      options: { mode: 'video', quality: { type: 'maxHeight', height: 1080 }, container: 'mp4' },
      context: {
        url: 'https://cdn.example.com/hls/master.m3u8',
        kind: 'stream',
        source: {
          pageUrl: 'https://dizi.example.com/izle/5',
          referer: 'https://player.example.com/',
          origin: 'https://player.example.com',
          userAgent: 'Mozilla/5.0',
        },
        title: 'Bölüm 5',
        thumbnail: 'https://cdn.example.com/poster.jpg',
      },
    });
  });

  it('takes a sound as a sound, without the video container', () => {
    const { options } = homeFromHandoff({ ...STREAM, kind: 'audio' }, DEFAULTS);
    expect(options.mode).toBe('audio');
    expect(options.container).toBeNull();
  });

  it('keeps the container when the default is already sound', () => {
    const defaults = { ...DEFAULTS, defaultMode: 'audio' as const, defaultContainer: 'm4a' };
    const { options } = homeFromHandoff({ ...STREAM, kind: 'video' }, defaults);
    expect(options.mode).toBe('audio');
    expect(options.container).toBe('m4a');
  });

  it('carries no empty strings into the headers or the title', () => {
    const { context } = homeFromHandoff(
      { ...STREAM, title: '  ', referer: '', origin: null, thumbnail: '' },
      DEFAULTS,
    );
    expect(context.title).toBeNull();
    expect(context.thumbnail).toBeNull();
    expect(context.source.referer).toBeNull();
    expect(context.source.origin).toBeNull();
  });
});

describe('titledFromHandoff', () => {
  const analysed = { title: 'master', thumbnailUrl: null as string | null };

  it('names a stream after the tab it played in', () => {
    const { context } = homeFromHandoff(STREAM, DEFAULTS);
    expect(titledFromHandoff(analysed, context)).toEqual({
      title: 'Bölüm 5',
      thumbnailUrl: 'https://cdn.example.com/poster.jpg',
    });
  });

  it("leaves a known site's own title alone, and only fills a missing picture", () => {
    const { context } = homeFromHandoff({ ...STREAM, kind: 'page' }, DEFAULTS);
    const page = { title: 'Radiohead - Creep', thumbnailUrl: 'https://i.ytimg.com/x.jpg' };
    expect(titledFromHandoff(page, context)).toBe(page);
    expect(titledFromHandoff({ ...page, thumbnailUrl: null }, context).thumbnailUrl).toBe(
      'https://cdn.example.com/poster.jpg',
    );
  });

  it('changes nothing for a link that was not handed over', () => {
    expect(titledFromHandoff(analysed, null)).toBe(analysed);
  });
});

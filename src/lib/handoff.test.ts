import { describe, expect, it } from 'vitest';

import type { Handoff } from '@/types';

import { requestFromHandoff } from './handoff';

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

describe('requestFromHandoff', () => {
  it('asks for what Home would with the default options', () => {
    expect(requestFromHandoff(STREAM, DEFAULTS)).toEqual({
      url: 'https://cdn.example.com/hls/master.m3u8',
      mode: 'video',
      quality: { type: 'maxHeight', height: 1080 },
      videoFormatId: null,
      audioFormatId: null,
      container: 'mp4',
      watermark: 'any',
      outputDir: null,
      title: 'Bölüm 5',
      thumbnailUrl: 'https://cdn.example.com/poster.jpg',
      platform: null,
      audioLanguage: null,
      source: {
        pageUrl: 'https://dizi.example.com/izle/5',
        referer: 'https://player.example.com/',
        origin: 'https://player.example.com',
        userAgent: 'Mozilla/5.0',
      },
    });
  });

  it('takes a sound as a sound, without the video container', () => {
    const request = requestFromHandoff({ ...STREAM, kind: 'audio' }, DEFAULTS);
    expect(request.mode).toBe('audio');
    expect(request.container).toBeNull();
  });

  it('keeps the container when the default is already sound', () => {
    const defaults = { ...DEFAULTS, defaultMode: 'audio' as const, defaultContainer: 'm4a' };
    const request = requestFromHandoff({ ...STREAM, kind: 'video' }, defaults);
    expect(request.mode).toBe('audio');
    expect(request.container).toBe('m4a');
  });

  it('leaves out what the browser did not know', () => {
    const request = requestFromHandoff(
      {
        ...STREAM,
        kind: 'page',
        title: '   ',
        referer: null,
        origin: null,
        userAgent: '',
        thumbnail: null,
      },
      DEFAULTS,
    );
    expect(request.title).toBeNull();
    expect(request.thumbnailUrl).toBeNull();
    expect(request.source).toEqual({
      pageUrl: 'https://dizi.example.com/izle/5',
      referer: null,
      origin: null,
      userAgent: null,
    });
  });
});

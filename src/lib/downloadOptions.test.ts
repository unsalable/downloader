import { describe, expect, it } from 'vitest';

import type { MediaFormat, MediaMetadata } from '@/types';

import { audioStreamOptions, videoStreamOptions } from './downloadOptions';

function stream(id: string, overrides: Partial<MediaFormat>): MediaFormat {
  return {
    id,
    kind: 'muxed',
    container: 'mp4',
    protocol: 'https',
    hasVideo: true,
    hasAudio: true,
    width: null,
    height: null,
    fps: null,
    vcodec: null,
    acodec: null,
    tbr: null,
    vbr: null,
    abr: null,
    filesize: null,
    filesizeApprox: null,
    qualityLabel: 'Original',
    watermarked: null,
    note: null,
    needsEngineDownload: false,
    language: null,
    languagePreference: null,
    ...overrides,
  };
}

function media(formats: MediaFormat[]): MediaMetadata {
  return { formats } as unknown as MediaMetadata;
}

const hevc = (id: string) =>
  stream(id, {
    kind: 'video',
    hasAudio: false,
    width: 1080,
    height: 1080,
    vcodec: 'h265',
    tbr: 1511,
    vbr: 1511,
    filesize: 36_463_968,
    qualityLabel: '1080p',
    watermarked: false,
  });

const h264 = (id: string) =>
  stream(id, {
    width: 720,
    height: 720,
    vcodec: 'h264',
    acodec: 'aac',
    tbr: 1207,
    filesize: 37_655_063,
    qualityLabel: '720p',
    watermarked: false,
  });

/**
 * TikTok as the engine listed it on 2026-10-08, cut down to what these menus
 * turn on: the stamped rendition first, then the 1080p picture and the 720p
 * rendition with sound, each once per CDN, and no sound kept apart.
 */
const TIKTOK = media([
  stream('download', {
    vcodec: 'h264',
    acodec: 'aac',
    qualityLabel: 'watermarked',
    watermarked: true,
  }),
  hevc('bytevc1_1080p_1511769-0'),
  hevc('bytevc1_1080p_1511769-1'),
  h264('h264_720p_1207492-0'),
  h264('h264_720p_1207492-1'),
]);

const values = (options: { value: string }[]) => options.map((option) => option.value);

describe('audioStreamOptions', () => {
  it('offers the sound inside a rendition when the source keeps none apart', () => {
    const options = audioStreamOptions(TIKTOK);
    // One entry for the 720p rendition, in the copy the plan takes: the last
    // listed. The stamped rendition is not offered while a clean one has sound.
    expect(values(options)).toEqual(['', 'h264_720p_1207492-1']);
    expect(options[1]).toMatchObject({
      label: 'Sound of the 720p video',
      description: 'AAC',
      meta: '35.9 MB',
    });
  });

  it('names the copy the plan took, whichever one it is', () => {
    expect(values(audioStreamOptions(TIKTOK, 'h264_720p_1207492-1'))).toEqual([
      '',
      'h264_720p_1207492-1',
    ]);
    expect(values(audioStreamOptions(TIKTOK, 'h264_720p_1207492-0'))).toEqual([
      '',
      'h264_720p_1207492-0',
    ]);
  });

  it('lists a stamped rendition that was chosen beside the clean one', () => {
    expect(values(audioStreamOptions(TIKTOK, 'download'))).toEqual([
      '',
      'h264_720p_1207492-1',
      'download',
    ]);
  });

  it('offers a stamped rendition when nothing clean has sound', () => {
    const options = audioStreamOptions(
      media(TIKTOK.formats.filter((format) => !format.id.startsWith('h264_'))),
    );
    expect(values(options)).toEqual(['', 'download']);
  });

  it('never borrows when the source keeps its sound apart', () => {
    const options = audioStreamOptions(
      media([
        h264('18'),
        stream('140', {
          kind: 'audio',
          hasVideo: false,
          acodec: 'mp4a.40.2',
          abr: 129.5,
          qualityLabel: '130 kbps',
        }),
      ]),
      '140',
    );
    expect(values(options)).toEqual(['', '140']);
  });

  it('lends nothing where every picture has its own sound, until a sound is taken alone', () => {
    // Every rendition carries both, as on most sites without separate streams.
    const formats = media([h264('22'), { ...h264('37'), height: 1080, qualityLabel: '1080p' }]);
    expect(values(audioStreamOptions(formats))).toEqual(['']);
    // Audio mode takes the sound out of one of them, and then each can lend it.
    expect(values(audioStreamOptions(formats, '37'))).toEqual(['', '22', '37']);
  });

  it('does not offer a rendition that does not name its sound, unless it was chosen', () => {
    const formats = media([hevc('v'), stream('http-720', { height: 720, qualityLabel: '720p' })]);
    expect(values(audioStreamOptions(formats))).toEqual(['']);
    expect(values(audioStreamOptions(formats, 'http-720'))).toEqual(['', 'http-720']);
  });
});

describe('videoStreamOptions', () => {
  it('lists each rendition once, in the copy the plan took', () => {
    // The plan takes the last of two equal copies; the menu has to name it.
    expect(values(videoStreamOptions(TIKTOK, 'bytevc1_1080p_1511769-1'))).toEqual([
      'download',
      'bytevc1_1080p_1511769-1',
      'h264_720p_1207492-1',
    ]);
    // A copy picked by hand stays picked.
    expect(values(videoStreamOptions(TIKTOK, 'bytevc1_1080p_1511769-0'))).toEqual([
      'download',
      'bytevc1_1080p_1511769-0',
      'h264_720p_1207492-1',
    ]);
  });

  it('says which renditions bring their own sound, in the language shown', () => {
    const [stamped, picture, both] = videoStreamOptions(TIKTOK);
    expect(stamped?.label).toBe('watermarked + audio');
    expect(picture?.label).toBe('1080p');
    expect(both?.label).toBe('720p + audio');
  });

  it('keeps renditions apart that read differently', () => {
    const options = videoStreamOptions(
      media([h264('a'), { ...h264('b'), tbr: 2400, filesize: 74_000_000 }]),
    );
    expect(values(options)).toEqual(['a', 'b']);
  });

  it('keeps renditions apart whose sound is in another language', () => {
    const options = videoStreamOptions(
      media([{ ...h264('hls-fr'), language: 'fr' }, { ...h264('hls-de'), language: 'de' }]),
    );
    expect(values(options)).toEqual(['hls-fr', 'hls-de']);
  });
});

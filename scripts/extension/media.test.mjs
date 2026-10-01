// Unit tests for extension/media.js, the extension's pure half.
//
//   node --test scripts/extension/
//
// They live outside extension/ so the store package never carries them.
// Vitest's default pattern also picks up every *.test.mjs in the repository,
// so the file takes its `describe`/`test` from whichever runner loaded it; the
// assertions are node:assert either way.

import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

import {
  PAYLOAD_FIELDS,
  PROTECTED,
  UNLISTED_SITES,
  classify,
  cleanTitle,
  dedupeKey,
  formatDuration,
  formatEstimate,
  formatSize,
  headerMap,
  isNoise,
  isoDuration,
  normaliseFrame,
  parseDash,
  parseHls,
  protectedService,
  refererFor,
  rows,
  siteOf,
  sizeFrom,
  toPayload,
  tooSmall,
} from '../../extension/media.js';

const { describe, test } = process.env.VITEST ? await import('vitest') : await import('node:test');

const repo = join(dirname(fileURLToPath(import.meta.url)), '..', '..');

const xhr = (url, contentType = '') => classify({ url, contentType, type: 'xmlhttprequest' });
const media = (url, contentType = '') => classify({ url, contentType, type: 'media' });

const UA = 'Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/141.0.0.0 Safari/537.36';

let nextId = 0;
function item(url, kind, extra = {}) {
  nextId += 1;
  return {
    id: `i${nextId}`,
    url,
    kind,
    frameId: 0,
    initiator: '',
    isXhr: false,
    contentType: '',
    size: null,
    seenOn: '',
    at: nextId,
    ...extra,
  };
}

function frame(raw, frameId = 0) {
  return normaliseFrame({ isTop: frameId === 0, videos: [], audios: [], ...raw }, frameId);
}

describe('classify', () => {
  test('HLS by content type, whatever the address says', () => {
    assert.equal(xhr('https://cdn.example/play?id=1', 'application/vnd.apple.mpegurl'), 'hls');
    assert.equal(xhr('https://cdn.example/play?id=1', 'application/x-mpegURL; charset=utf-8'), 'hls');
    assert.equal(xhr('https://cdn.example/play', 'AUDIO/MPEGURL'), 'hls');
    assert.equal(xhr('https://cdn.example/play', 'audio/x-mpegurl'), 'hls');
  });

  test('HLS by extension, even when served as text', () => {
    assert.equal(xhr('https://cdn.example/hls/master.m3u8?token=abc', 'text/plain'), 'hls');
    assert.equal(xhr('https://cdn.example/hls/master.M3U8'), 'hls');
    assert.equal(xhr('https://radio.example/live.m3u', 'audio/x-mpegurl'), 'hls');
  });

  test('DASH and Smooth Streaming manifests', () => {
    assert.equal(xhr('https://cdn.example/manifest', 'application/dash+xml'), 'dash');
    assert.equal(xhr('https://cdn.example/dash/stream.mpd'), 'dash');
    assert.equal(xhr('https://cdn.example/vod/film.ism/Manifest'), 'dash');
  });

  test('segments are recognised and never become rows', () => {
    const segments = [
      ['https://cdn.example/hls/720p/00042.ts', ''],
      ['https://cdn.example/x/clip', 'video/mp2t'],
      ['https://cdn.example/x/clip', 'video/iso.segment'],
      ['https://cdn.example/dash/chunk-stream0-00001.m4s', 'video/mp4'],
      ['https://cdn.example/a.m4f', ''],
      ['https://cdn.example/a.cmfv', ''],
      ['https://cdn.example/a.cmfa', ''],
      ['https://cdn.example/hls/seg-12.mp4', 'video/mp4'],
      ['https://cdn.example/hls/segment12.mp4', 'video/mp4'],
      ['https://cdn.example/hls/segment-12-v1-a1.mp4', 'video/mp4'],
      ['https://cdn.example/hls/chunk_3.m4a', 'audio/mp4'],
      ['https://cdn.example/hls/frag5.mp4', 'video/mp4'],
      ['https://cdn.example/hls/fragment-1.mp4', 'video/mp4'],
      ['https://cdn.example/hls/part3.mp4', 'video/mp4'],
      ['https://cdn.example/dash/init.mp4', 'video/mp4'],
      ['https://cdn.example/dash/init-v1.mp4', 'video/mp4'],
      ['https://cdn.example/hls/000123.mp4', 'video/mp4'],
      ['https://cdn.example/hls/fileSequence12.aac', 'audio/aac'],
    ];
    for (const [url, type] of segments) assert.equal(xhr(url, type), 'segment', url);
  });

  test('a name that only contains a segment word is a file', () => {
    assert.equal(media('https://cdn.example/lecture-part3.mp4', 'video/mp4'), 'video');
    // A media element loading a file named by its id is loading the file.
    assert.equal(media('https://cdn.example/videos/98234.mp4', 'video/mp4'), 'video');
  });

  test('files by type and by extension', () => {
    assert.equal(xhr('https://cdn.example/watch', 'video/mp4'), 'video');
    assert.equal(xhr('https://cdn.example/clip.webm'), 'video');
    assert.equal(xhr('https://cdn.example/clip.MOV', 'video/quicktime'), 'video');
    assert.equal(xhr('https://cdn.example/song', 'audio/mpeg'), 'audio');
    assert.equal(xhr('https://cdn.example/song.flac'), 'audio');
    assert.equal(xhr('https://cdn.example/song.opus', 'audio/ogg'), 'audio');
  });

  test('octet-stream counts only with a media extension', () => {
    assert.equal(xhr('https://bucket.example/file.mp4', 'application/octet-stream'), 'video');
    assert.equal(xhr('https://bucket.example/file.mp3', 'binary/octet-stream'), 'audio');
    assert.equal(xhr('https://bucket.example/blob', 'application/octet-stream'), null);
    // ...or when a media element is the one loading it.
    assert.equal(media('https://bucket.example/blob', 'application/octet-stream'), 'video');
  });

  test('a media extension answered with something else is not media', () => {
    assert.equal(xhr('https://site.example/video.mp4', 'text/html'), null);
    assert.equal(xhr('https://site.example/poster.jpg', 'image/jpeg'), null);
    assert.equal(xhr('https://site.example/api/list', 'application/json'), null);
  });

  test('YouTube playback, byte ranges and Vimeo ranges are noise', () => {
    const youtube =
      'https://rr3---sn-4g5edndl.googlevideo.com/videoplayback?expire=1&itag=399&mime=video%2Fmp4';
    assert.equal(xhr(youtube, 'application/vnd.yt-ump'), 'noise');
    assert.equal(xhr(`${youtube}&range=0-1000`, 'video/mp4'), 'noise');
    assert.equal(
      xhr('https://video-ist1-1.xx.fbcdn.net/v/t42.1790-2/abc.mp4?_nc_cat=1&bytestart=0&byteend=99999', 'video/mp4'),
      'noise',
    );
    assert.equal(
      xhr('https://vod-adaptive-ak.vimeocdn.com/exp=1~acl=x/range/prot/abc.mp4?pathsig=1', 'video/mp4'),
      'noise',
    );
    assert.equal(isNoise(youtube), true);
  });

  test('advertising is noise, wherever it plays from', () => {
    const ads = [
      'https://s0.2mdn.net/videoplayback/123/ad.mp4',
      'https://pubads.g.doubleclick.net/gampad/ad.mp4',
      'https://imasdk.googleapis.com/js/media/creative.mp4',
      'https://cdn.example/ads/preroll.mp4',
      'https://cdn.example/vast/creative.m3u8',
      'https://c.amazon-adsystem.com/clip.webm',
    ];
    for (const url of ads) assert.equal(xhr(url, 'video/mp4'), 'noise', url);
    // An advert host's script is not media at all, so it says nothing.
    assert.equal(xhr('https://securepubads.g.doubleclick.net/tag/js/gpt.js', 'text/javascript'), null);
  });

  test('anything but http and https is ignored', () => {
    assert.equal(xhr('blob:https://site.example/1234', 'video/mp4'), null);
    assert.equal(xhr('data:video/mp4;base64,AAAA', 'video/mp4'), null);
    assert.equal(xhr('not a url', 'video/mp4'), null);
  });
});

describe('sizes', () => {
  test('the total after the slash of Content-Range wins', () => {
    const headers = headerMap([
      { name: 'Content-Range', value: 'bytes 0-1023/52428800' },
      { name: 'Content-Length', value: '1024' },
    ]);
    assert.equal(sizeFrom(headers, 206), 52428800);
  });

  test('a 206 without Content-Range says nothing about the file', () => {
    assert.equal(sizeFrom(headerMap([{ name: 'content-length', value: '1024' }]), 206), null);
  });

  test('Content-Length on a whole response', () => {
    assert.equal(sizeFrom(headerMap([{ name: 'Content-Length', value: '734003' }]), 200), 734003);
    assert.equal(sizeFrom({}, 200), null);
  });

  test('tiny files are dropped, unknown sizes are not', () => {
    assert.equal(tooSmall('video', 200 * 1024), true);
    assert.equal(tooSmall('video', 400 * 1024), false);
    assert.equal(tooSmall('audio', 50 * 1024), true);
    assert.equal(tooSmall('audio', 100 * 1024), false);
    assert.equal(tooSmall('video', null), false);
    assert.equal(tooSmall('hls', 900), false);
  });
});

describe('parseHls', () => {
  const MASTER = `#EXTM3U
#EXT-X-VERSION:6
#EXT-X-MEDIA:TYPE=AUDIO,GROUP-ID="aud",NAME="English, original",DEFAULT=YES,URI="audio/en.m3u8"
#EXT-X-STREAM-INF:BANDWIDTH=800000,RESOLUTION=640x360,CODECS="avc1.4d401e,mp4a.40.2",AUDIO="aud"
360p/index.m3u8
#EXT-X-STREAM-INF:BANDWIDTH=5000000,RESOLUTION=1920x1080,AUDIO="aud"
https://other.example/1080p/index.m3u8?token=a

#EXT-X-STREAM-INF:BANDWIDTH=2500000,RESOLUTION=1280x720
../720p/index.m3u8
#EXT-X-I-FRAME-STREAM-INF:BANDWIDTH=90000,RESOLUTION=1920x1080,URI="iframes.m3u8"
`;

  test('a master gives its tallest resolution and every URL it names', () => {
    const info = parseHls(MASTER, 'https://cdn.example/hls/show/master.m3u8');
    assert.equal(info.master, true);
    assert.equal(info.height, 1080);
    assert.equal(info.protected, false);
    assert.deepEqual(info.variants.sort(), [
      'https://cdn.example/hls/720p/index.m3u8',
      'https://cdn.example/hls/show/360p/index.m3u8',
      'https://cdn.example/hls/show/audio/en.m3u8',
      'https://cdn.example/hls/show/iframes.m3u8',
      'https://other.example/1080p/index.m3u8?token=a',
    ]);
  });

  test('a finished media playlist gives its length', () => {
    const text = '#EXTM3U\r\n#EXT-X-TARGETDURATION:10\r\n#EXTINF:10.0,\r\na.ts\r\n#EXTINF:9.5,\r\nb.ts\r\n#EXTINF:4.25,title\r\nc.ts\r\n#EXT-X-ENDLIST\r\n';
    assert.deepEqual(parseHls(text, 'https://cdn.example/v.m3u8'), {
      master: false,
      durationSec: 23.75,
      protected: false,
    });
  });

  test('a live window is not a duration', () => {
    const text = '#EXTM3U\n#EXT-X-MEDIA-SEQUENCE:900\n#EXTINF:6,\na.ts\n#EXTINF:6,\nb.ts\n';
    assert.equal(parseHls(text, 'https://cdn.example/live.m3u8').durationSec, null);
  });

  test('AES-128 is plain encryption, not DRM', () => {
    const text =
      '#EXTM3U\n#EXT-X-KEY:METHOD=AES-128,URI="https://keys.example/k.bin",IV=0x0123456789abcdef0123456789abcdef\n#EXTINF:6,\na.ts\n#EXT-X-ENDLIST\n';
    assert.equal(parseHls(text, 'https://cdn.example/v.m3u8').protected, false);
  });

  test('SAMPLE-AES, Widevine and FairPlay keys are protected', () => {
    const keys = [
      '#EXT-X-KEY:METHOD=SAMPLE-AES,URI="https://keys.example/k",KEYFORMAT="identity"',
      '#EXT-X-KEY:METHOD=SAMPLE-AES-CTR,KEYFORMAT="urn:uuid:edef8ba9-79d6-4ace-a3c8-27dcd51d21ed",URI="data:text/plain;base64,AAAA"',
      '#EXT-X-KEY:METHOD=AES-128,URI="skd://twelve-key-id"',
      '#EXT-X-KEY:METHOD=ISO-23001-7,KEYFORMAT="com.microsoft.playready"',
    ];
    for (const key of keys) {
      const text = `#EXTM3U\n${key}\n#EXTINF:6,\na.ts\n#EXT-X-ENDLIST\n`;
      assert.equal(parseHls(text, 'https://cdn.example/v.m3u8').protected, true, key);
    }
  });

  test('a session key in the master marks the whole stream', () => {
    const text = `#EXTM3U
#EXT-X-SESSION-KEY:METHOD=SAMPLE-AES,URI="skd://abc",KEYFORMAT="com.apple.streamingkeydelivery",KEYFORMATVERSIONS="1"
#EXT-X-STREAM-INF:BANDWIDTH=1,RESOLUTION=1280x720
v.m3u8
`;
    const info = parseHls(text, 'https://cdn.example/m.m3u8');
    assert.equal(info.master, true);
    assert.equal(info.protected, true);
  });

  test('something that is not a playlist', () => {
    assert.equal(parseHls('<html>Not found</html>', 'https://cdn.example/m.m3u8'), null);
    assert.equal(parseHls(null, 'https://cdn.example/m.m3u8'), null);
  });
});

describe('parseDash', () => {
  const MPD = `<?xml version="1.0" encoding="UTF-8"?>
<MPD xmlns="urn:mpeg:dash:schema:mpd:2011" type="static" mediaPresentationDuration="PT23M41.2S" minBufferTime="PT2S">
  <BaseURL>https://cdn.example/dash/</BaseURL>
  <Period>
    <AdaptationSet mimeType="video/mp4" maxHeight="1080">
      <Representation id="1" bandwidth="1" width="1280" height="720"><BaseURL>video_720.mp4</BaseURL></Representation>
      <Representation id="2" bandwidth="2" width="1920" height="1080"><BaseURL>video_1080.mp4?a=1&amp;b=2</BaseURL></Representation>
    </AdaptationSet>
    <AdaptationSet mimeType="audio/mp4">
      <Representation id="3" bandwidth="3"><BaseURL>audio.mp4</BaseURL></Representation>
    </AdaptationSet>
  </Period>
</MPD>`;

  test('height, duration and the files it names', () => {
    const info = parseDash(MPD, 'https://site.example/manifest.mpd');
    assert.equal(info.height, 1080);
    assert.equal(info.durationSec, 1421.2);
    assert.equal(info.protected, false);
    assert.ok(info.variants.includes('https://cdn.example/dash/video_1080.mp4?a=1&b=2'));
    assert.ok(info.variants.includes('https://cdn.example/dash/audio.mp4'));
  });

  test('any ContentProtection is protected', () => {
    const text = MPD.replace(
      '<AdaptationSet mimeType="video/mp4" maxHeight="1080">',
      '<AdaptationSet mimeType="video/mp4"><ContentProtection schemeIdUri="urn:uuid:edef8ba9-79d6-4ace-a3c8-27dcd51d21ed"/>',
    );
    assert.equal(parseDash(text, 'https://site.example/m.mpd').protected, true);
  });

  test('a live manifest has no duration', () => {
    const text = '<MPD type="dynamic" mediaPresentationDuration="PT1H"><Period/></MPD>';
    assert.equal(parseDash(text, 'https://site.example/m.mpd').durationSec, null);
  });

  test('Smooth Streaming', () => {
    const text =
      '<SmoothStreamingMedia MajorVersion="2" Duration="14212000000" TimeScale="10000000"><StreamIndex Type="video"><QualityLevel MaxHeight="720"/></StreamIndex><Protection><ProtectionHeader/></Protection></SmoothStreamingMedia>';
    const info = parseDash(text, 'https://site.example/film.ism/Manifest');
    assert.equal(info.height, 720);
    assert.equal(info.durationSec, 1421.2);
    assert.equal(info.protected, true);
  });

  test('ISO 8601 durations', () => {
    assert.equal(isoDuration('PT1H2M3.5S'), 3723.5);
    assert.equal(isoDuration('PT45S'), 45);
    assert.equal(isoDuration('P0Y0M0DT0H3M0S'), 180);
    assert.equal(isoDuration('P1DT2H'), 93600);
    assert.equal(isoDuration('PT0S'), null);
    assert.equal(isoDuration('soon'), null);
    assert.equal(isoDuration(null), null);
  });

  test('something that is not a manifest', () => {
    assert.equal(parseDash('<html></html>', 'https://site.example/m.mpd'), null);
  });
});

describe('rows', () => {
  const PAGE = 'https://site.example/watch/5';

  test('the variants of a parsed master are part of its row', () => {
    const master = item('https://cdn.example/hls/master.m3u8', 'hls', {
      info: { master: true, variants: ['https://cdn.example/hls/720p.m3u8', 'https://cdn.example/hls/audio.m3u8'], height: 720, protected: false },
    });
    const variant = item('https://cdn.example/hls/720p.m3u8', 'hls', { info: { master: false, durationSec: 60, protected: false } });
    const audio = item('https://cdn.example/hls/audio.m3u8', 'hls');
    const list = rows({ url: PAGE, title: 'Bölüm 5', sawMedia: true, items: [master, variant, audio] }, []);
    assert.equal(list.length, 1);
    assert.equal(list[0].url, 'https://cdn.example/hls/master.m3u8');
    assert.deepEqual(list[0].meta, { label: 'stream', parts: ['720p'] });
  });

  test('the files a DASH manifest names are part of its row', () => {
    const mpd = item('https://cdn.example/dash/manifest.mpd', 'dash', {
      info: { master: true, variants: ['https://cdn.example/dash/video_1080.mp4', 'https://cdn.example/dash/audio.mp4'], height: 1080, durationSec: 1421, protected: false },
    });
    const video = item('https://cdn.example/dash/video_1080.mp4', 'video', { size: 90e6 });
    const sound = item('https://cdn.example/dash/audio.mp4', 'audio', { size: 9e6 });
    const list = rows({ url: PAGE, title: 'T', items: [mpd, video, sound] }, []);
    assert.deepEqual(list.map((row) => row.kind), ['stream']);
    assert.deepEqual(list[0].meta.parts, ['1080p', '23:41']);
  });

  test('the page row on YouTube once something has played', () => {
    const state = {
      url: 'https://www.youtube.com/watch?v=dQw4w9WgXcQ&t=30s',
      title: '(3) Never Gonna Give You Up - YouTube',
      sawMedia: true,
      items: [],
    };
    const list = rows(state, [], { userAgent: UA });
    assert.equal(list.length, 1);
    const [page] = list;
    assert.equal(page.kind, 'page');
    assert.equal(page.title, 'Never Gonna Give You Up');
    assert.deepEqual(page.meta, { label: 'site', site: 'YouTube', parts: [] });
    assert.equal(page.thumbnail, 'https://i.ytimg.com/vi/dQw4w9WgXcQ/mqdefault.jpg');
    assert.equal(page.protected, false);
    assert.deepEqual(page.payload, {
      url: state.url,
      kind: 'page',
      title: 'Never Gonna Give You Up',
      pageUrl: state.url,
      userAgent: UA,
      thumbnail: 'https://i.ytimg.com/vi/dQw4w9WgXcQ/mqdefault.jpg',
    });
  });

  test('no page row on YouTube before anything plays, nor on its front page', () => {
    assert.deepEqual(rows({ url: 'https://www.youtube.com/watch?v=dQw4w9WgXcQ', sawMedia: false, items: [] }, []), []);
    assert.deepEqual(rows({ url: 'https://www.youtube.com/', sawMedia: true, items: [] }, []), []);
  });

  test('stale og: tags from an earlier video are not used', () => {
    const state = { url: 'https://www.youtube.com/watch?v=NEWNEWNEW01', title: '', sawMedia: true, items: [] };
    const top = frame({
      href: state.url,
      title: 'New video - YouTube',
      ogTitle: 'Old video',
      ogImage: 'https://i.ytimg.com/vi/OLDOLDOLD01/maxresdefault.jpg',
      ogUrl: 'https://www.youtube.com/watch?v=OLDOLDOLD01',
      videos: [{ src: '', duration: 212.4, height: 1080, drm: false }],
    });
    const [page] = rows(state, [top]);
    assert.equal(page.title, 'New video');
    assert.equal(page.thumbnail, 'https://i.ytimg.com/vi/NEWNEWNEW01/mqdefault.jpg');
    // What the player is playing may be the advert; the app says how long the video is.
    assert.deepEqual(page.meta.parts, []);
  });

  test('elsewhere, the page row only stands in for a player with nothing to list', () => {
    const state = { url: PAGE, title: '', sawMedia: true, items: [] };
    const top = frame({ href: PAGE, title: 'Bölüm 5', videos: [{ src: '', duration: 1421, drm: false }] });
    const list = rows(state, [top]);
    assert.equal(list.length, 1);
    assert.equal(list[0].kind, 'page');
    assert.equal(list[0].title, 'Bölüm 5');
    assert.deepEqual(list[0].meta, { label: 'page', parts: ['23:41'] });

    // Without an element there is nothing to say the page plays anything.
    assert.deepEqual(rows(state, [frame({ href: PAGE, title: 'Bölüm 5' })]), []);
    // With a real stream, the stream is the row.
    const withStream = rows({ ...state, items: [item('https://cdn.example/a.m3u8', 'hls')] }, [top]);
    assert.deepEqual(withStream.map((row) => row.kind), ['stream']);
  });

  test('a page whose player uses EME stands in as protected, with nothing to press', () => {
    // The manifest was never seen -- the tab was playing before the extension
    // started, say -- so the element is all there is to go on.
    const state = { url: 'https://drm.example/watch/1', title: 'Film', sawMedia: true, items: [] };
    const top = frame({
      href: state.url,
      title: 'Film',
      videos: [{ src: 'blob:https://drm.example/abc', duration: 3600, drm: true }],
    });
    const list = rows(state, [top]);
    assert.equal(list.length, 1);
    assert.equal(list[0].kind, 'page');
    assert.equal(list[0].protected, true);
    assert.deepEqual(list[0].meta, { label: 'protected', parts: [] });

    // The same in a player frame of its own.
    const player = frame({ href: 'https://player.example/embed/1', videos: [{ src: '', drm: true }] }, 4);
    const [embedded] = rows(state, [frame({ href: state.url, title: 'Film' }), player]);
    assert.equal(embedded.protected, true);
  });

  test('an unlisted site lists nothing, on its own pages or embedded elsewhere', () => {
    // What STORE-LISTING.md says to do if review objects to YouTube.
    const youtube = ['youtube.com', 'youtu.be', 'music.youtube.com'];
    const watch = 'https://www.youtube.com/watch?v=dQw4w9WgXcQ';
    const blobPlayer = { src: 'blob:https://www.youtube.com/abc', duration: 212, drm: false };
    const embed = frame({ href: 'https://www.youtube.com/embed/dQw4w9WgXcQ', videos: [blobPlayer] }, 6);
    const host = { url: PAGE, title: 'Bölüm 5', sawMedia: true, items: [] };

    // Listed today: the watch page is offered, and so is a page embedding it.
    assert.equal(rows({ url: watch, sawMedia: true, items: [] }, [frame({ href: watch, videos: [blobPlayer] })]).length, 1);
    assert.equal(rows(host, [frame({ href: PAGE }), embed]).length, 1);

    UNLISTED_SITES.push(...youtube);
    try {
      assert.deepEqual(rows({ url: watch, sawMedia: true, items: [] }, [frame({ href: watch, videos: [blobPlayer] })]), []);
      assert.deepEqual(rows({ url: 'https://youtu.be/dQw4w9WgXcQ', sawMedia: true, items: [] }, []), []);
      assert.deepEqual(rows(host, [frame({ href: PAGE }), embed]), []);
      const fromEmbed = item('https://manifest.googlevideo.com/api/manifest/hls_playlist/x/index.m3u8', 'hls', {
        initiator: 'https://www.youtube.com',
        frameId: 6,
      });
      assert.deepEqual(rows({ ...host, items: [fromEmbed] }, [frame({ href: PAGE }), embed]), []);
    } finally {
      UNLISTED_SITES.length = 0;
    }
  });

  test('an element playing a real file becomes a row, enriched when it was also fetched', () => {
    const top = frame({
      href: PAGE,
      title: 'Clip',
      videos: [{ src: 'https://cdn.example/clip.mp4', poster: 'https://cdn.example/poster.jpg', duration: 65, height: 720 }],
    });
    const [alone] = rows({ url: PAGE, items: [] }, [top]);
    assert.equal(alone.kind, 'video');
    assert.equal(alone.thumbnail, 'https://cdn.example/poster.jpg');
    assert.deepEqual(alone.meta.parts, ['720p', '1:05']);

    const fetched = item('https://cdn.example/clip.mp4', 'video', { size: 24 * 1024 * 1024 });
    const both = rows({ url: PAGE, items: [fetched] }, [top]);
    assert.equal(both.length, 1);
    assert.deepEqual(both[0].meta.parts, ['720p', '1:05']);
    assert.equal(both[0].size, 24 * 1024 * 1024);
  });

  test('a protected service lists nothing at all', () => {
    const items = [item('https://cdn.example/a.mpd', 'dash')];
    assert.deepEqual(rows({ url: 'https://www.netflix.com/watch/81234567', sawMedia: true, items }, []), []);
    assert.equal(protectedService('https://www.netflix.com/watch/1'), 'Netflix');
    assert.equal(protectedService('https://www.amazon.de/gp/video/detail/B0'), 'Prime Video');
    assert.equal(protectedService('https://www.amazon.de/dp/B0'), null);
    assert.equal(protectedService('https://music.apple.com/tr/album/1'), 'Apple Music');
    assert.equal(protectedService('https://www.apple.com/tv/'), null);
    assert.equal(protectedService('https://www.tvplus.com.tr/canli'), 'TV+');
  });

  test('protected streams and DRM frames say so and sink to the bottom', () => {
    const drm = item('https://cdn.example/drm.m3u8', 'hls', { info: { master: true, variants: [], height: 2160, protected: true } });
    const clear = item('https://cdn.example/clear.m3u8', 'hls', { info: { master: true, variants: [], height: 480, protected: false } });
    const inPlayer = item('https://player.example/eme.mpd', 'dash', { frameId: 7 });
    const player = frame({ href: 'https://player.example/embed/1', videos: [{ src: '', drm: true }] }, 7);
    const list = rows({ url: PAGE, items: [drm, clear, inPlayer] }, [frame({ href: PAGE }), player]);
    assert.deepEqual(
      list.map((row) => [row.url, row.protected]),
      [
        ['https://cdn.example/clear.m3u8', false],
        ['https://cdn.example/drm.m3u8', true],
        ['https://player.example/eme.mpd', true],
      ],
    );
    assert.deepEqual(list[1].meta, { label: 'protected', parts: [] });
  });

  test('order: streams by height, videos by size, audio, protected', () => {
    const items = [
      item('https://cdn.example/song.mp3', 'audio', { size: 5e6 }),
      item('https://cdn.example/small.mp4', 'video', { size: 10e6 }),
      item('https://cdn.example/big.mp4', 'video', { size: 50e6 }),
      item('https://cdn.example/locked.m3u8', 'hls', { info: { master: true, variants: [], height: 1080, protected: true } }),
      item('https://cdn.example/720.m3u8', 'hls', { info: { master: true, variants: [], height: 720, protected: false } }),
      item('https://cdn.example/1080.m3u8', 'hls', { info: { master: true, variants: [], height: 1080, protected: false } }),
      item('https://cdn.example/unknown.mpd', 'dash'),
    ];
    // Three players in the page, so their streams are three videos and none
    // is folded into another.
    const players = frame({ href: PAGE, videos: [{ src: '' }, { src: '' }, { src: '' }] });
    const list = rows({ url: PAGE, sawMedia: true, items }, [players]);
    assert.deepEqual(
      list.map((row) => row.url.replace('https://cdn.example/', '')),
      ['1080.m3u8', '720.m3u8', 'unknown.mpd', 'big.mp4', 'small.mp4', 'song.mp3', 'locked.m3u8'],
    );
    assert.deepEqual(list[3].meta, { label: 'video', parts: ['47.7 MB'] });
  });

  test('a video page on a known site is one row, the page, whatever its player fetched', () => {
    const items = [
      item('https://cdn.example/1080.m3u8', 'hls', { info: { master: true, variants: [], height: 1080, protected: false } }),
      item('https://cdn.example/big.mp4', 'video', { size: 50e6 }),
    ];
    const list = rows({ url: 'https://vimeo.com/123456', sawMedia: true, items }, []);
    assert.deepEqual(list.map((row) => row.url), ['https://vimeo.com/123456']);
    assert.deepEqual(list[0].meta, { label: 'site', site: 'Vimeo', parts: [] });
  });

  test('a feed on a known site is no page row; what it played is listed instead', () => {
    const items = [item('https://video.twimg.example/a.m3u8', 'hls', { info: { master: true, variants: [], height: 720, protected: false } })];
    const list = rows({ url: 'https://x.com/home', sawMedia: true, items }, []);
    assert.deepEqual(list.map((row) => row.kind), ['stream']);
    assert.deepEqual(rows({ url: 'https://x.com/someone/status/123', sawMedia: true, items }, []).map((row) => row.kind), ['page']);
  });

  test('one player fetching the same video twice is one row, the better described', () => {
    const items = [
      item('https://cdn.example/a/master.m3u8', 'hls', { info: { master: true, variants: [], height: 720, protected: false } }),
      item('https://cdn.example/a/manifest.mpd', 'dash', { info: { height: 1080, protected: false } }),
    ];
    const list = rows({ url: PAGE, sawMedia: true, items }, [frame({ href: PAGE, videos: [{ src: '' }] })]);
    assert.deepEqual(list.map((row) => row.url), ['https://cdn.example/a/manifest.mpd']);
  });

  test('the og: tags YouTube leaves behind from its front page are not this video', () => {
    const watch = 'https://www.youtube.com/watch?v=XFkzRNyygfk';
    const top = frame({
      href: watch,
      title: '(3) Radiohead - Creep - YouTube',
      ogTitle: 'YouTube',
      ogImage: 'https://www.youtube.com/img/desktop/yt_1200.png',
      ogUrl: '',
      videos: [{ src: '', duration: 238 }],
    });
    const [row] = rows({ url: watch, sawMedia: true, items: [] }, [top]);
    assert.equal(row.title, 'Radiohead - Creep');
    assert.equal(row.thumbnail, 'https://i.ytimg.com/vi/XFkzRNyygfk/mqdefault.jpg');
    assert.equal(row.payload.title, 'Radiohead - Creep');
  });

  test("Vimeo's own title suffix goes as the others' do", () => {
    const url = 'https://vimeo.com/76979871';
    const top = frame({ href: url, title: 'The New Vimeo Player on Vimeo', videos: [{ src: '' }] });
    assert.equal(rows({ url, sawMedia: true, items: [] }, [top])[0].title, 'The New Vimeo Player');
  });

  test('Referer and Origin are what the browser sent, near enough', () => {
    const own = item('https://cdn.example/own.m3u8', 'hls', { initiator: 'https://site.example', isXhr: true });
    const embedded = item('https://cdn.example/embed.m3u8', 'hls', { initiator: 'https://player.example', isXhr: true, frameId: 3 });
    const element = item('https://cdn.example/file.mp4', 'video', { initiator: 'https://player.example', isXhr: false });
    const unknown = item('https://cdn.example/unknown.m3u8', 'hls', { frameId: 5 });
    const list = rows({ url: PAGE, title: 'Bölüm 5', items: [own, embedded, element, unknown] }, [], { userAgent: UA });
    const by = (url) => list.find((row) => row.url === url).payload;

    assert.equal(by(own.url).referer, PAGE);
    assert.equal(by(own.url).origin, 'https://site.example');
    assert.equal(by(embedded.url).referer, 'https://player.example/');
    assert.equal(by(embedded.url).origin, 'https://player.example');
    assert.equal(by(element.url).referer, 'https://player.example/');
    assert.equal('origin' in by(element.url), false);
    assert.equal(by(unknown.url).referer, PAGE);
    assert.equal('origin' in by(unknown.url), false);

    assert.equal(refererFor('null', PAGE), PAGE);
    assert.equal(refererFor('chrome-extension://abc', PAGE), PAGE);
  });

  test("payloads use Contract A's field names exactly", () => {
    const stream = item('https://cdn.example/hls/master.m3u8', 'hls', { initiator: 'https://player.example', isXhr: true });
    const top = frame({
      href: PAGE,
      title: 'Bölüm 5 - Site',
      ogTitle: 'Bölüm 5',
      ogImage: 'https://site.example/poster.jpg',
    });
    const [row] = rows({ url: PAGE, items: [stream] }, [top], { userAgent: UA });
    assert.deepEqual(row.payload, {
      url: 'https://cdn.example/hls/master.m3u8',
      kind: 'stream',
      title: 'Bölüm 5',
      pageUrl: PAGE,
      referer: 'https://player.example/',
      origin: 'https://player.example',
      userAgent: UA,
      thumbnail: 'https://site.example/poster.jpg',
    });
    assert.deepEqual(Object.keys(row.payload), PAYLOAD_FIELDS);
    for (const key of Object.keys(row.payload)) assert.ok(PAYLOAD_FIELDS.includes(key), key);
  });

  test('no more than thirty rows', () => {
    const items = Array.from({ length: 45 }, (_, n) => item(`https://cdn.example/v${n}.mp4`, 'video', { size: 1e6 + n }));
    assert.equal(rows({ url: PAGE, items }, []).length, 30);
  });
});

describe('payload, titles and formats', () => {
  test('toPayload keeps only what the host accepts', () => {
    assert.deepEqual(
      toPayload({
        url: 'https://cdn.example/a.m3u8',
        kind: 'weird',
        title: '  Bölüm\u0000 5\n ',
        origin: 'https://player.example/path',
        referer: 'javascript:alert(1)',
        cookies: 'never',
        userAgent: 'agent\u0007',
      }),
      { url: 'https://cdn.example/a.m3u8', kind: 'page', title: 'Bölüm 5' },
    );
    assert.equal(toPayload({ url: 'file:///C:/x.mp4', kind: 'video' }), null);
    assert.equal(toPayload(null), null);
    assert.equal(toPayload({ url: `https://cdn.example/${'a'.repeat(9000)}`, kind: 'video' }), null);
  });

  test('cleanTitle', () => {
    assert.equal(cleanTitle('(12) Song\tname\r\n'), 'Song name');
    assert.equal(cleanTitle(undefined), '');
    assert.equal(Array.from(cleanTitle('ş'.repeat(400))).length, 300);
  });

  test('dedupeKey drops anchors and byte ranges', () => {
    assert.equal(
      dedupeKey('https://cdn.example/a.mp4?token=1&range=0-100&bytestart=0&byteend=9#t=10'),
      'https://cdn.example/a.mp4?token=1',
    );
    assert.equal(dedupeKey('blob:https://site.example/1'), '');
  });

  test('sites the app reads by page address', () => {
    assert.equal(siteOf('https://music.youtube.com/watch?v=1'), 'YouTube Music');
    assert.equal(siteOf('https://m.youtube.com/watch?v=1'), 'YouTube');
    assert.equal(siteOf('https://artist.bandcamp.com/track/x'), 'Bandcamp');
    assert.equal(siteOf('https://www.pinterest.co.uk/pin/1'), 'Pinterest');
    assert.equal(siteOf('https://notyoutube.com/watch'), null);
  });

  test('formatSize and formatDuration match the app', () => {
    assert.equal(formatSize(24 * 1024 * 1024), '24.0 MB');
    assert.equal(formatSize(85458944), '81.5 MB');
    assert.equal(formatSize(4.2 * 1024 * 1024), '4.20 MB');
    assert.equal(formatSize(1.5 * 1024 * 1024 * 1024), '1.50 GB');
    assert.equal(formatEstimate(31 * 1024 * 1024), '~31 MB');
    assert.equal(formatEstimate(1.24 * 1024 * 1024 * 1024), '~1.2 GB');
    assert.equal(formatEstimate(0), '');
    assert.equal(formatSize(512 * 1024), '512 KB');
    assert.equal(formatDuration(1421), '23:41');
    assert.equal(formatDuration(65), '1:05');
    assert.equal(formatDuration(3723), '1:02:03');
    assert.equal(formatDuration(0), '');
  });

  test("PROTECTED mirrors the app's list in detect.rs", () => {
    const source = readFileSync(join(repo, 'src-tauri', 'src', 'providers', 'detect.rs'), 'utf8');
    const block = /const PROTECTED: &\[\(&str, &str\)\] = &\[([\s\S]*?)\];/.exec(source);
    assert.ok(block, 'PROTECTED not found in detect.rs');
    const pairs = [...block[1].matchAll(/\("([^"]+)",\s*"([^"]+)"\)/g)].map((m) => [m[1], m[2]]);
    assert.deepEqual(PROTECTED, pairs);
  });
});

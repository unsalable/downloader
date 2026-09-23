/**
 * A stand-in for the Tauri backend, for looking at the interface in a browser.
 * Injected by vite.config.mjs in this folder; never part of a real build.
 *
 * Switches, as query parameters:
 *   ?platform=android   the phone layout (default: windows)
 *   ?theme=light        start in the light theme (default: dark)
 *   ?lang=en            start in English (default: tr)
 *   ?empty=1            no downloads, conversions or history
 *   ?welcome=1          show the first-run screen
 *   ?update=1           pretend a newer build has been released
 *
 * From the console, `__UD_MOCK__.emit(event, payload)` delivers a backend
 * event and `__UD_MOCK__.calls` lists every command the app has invoked.
 * `__UD_MOCK__.picked` is what the phone's file picker returns next.
 */
(() => {
  const params = new URLSearchParams(location.search);
  const platform = params.get('platform') ?? 'windows';
  const empty = params.has('empty');
  const now = Date.now();

  // -- fixtures --------------------------------------------------------------

  const settings = {
    startWithWindows: false,
    minimizeToTray: true,
    closeToTray: true,
    clipboardMonitoring: false,
    notificationsEnabled: true,
    notifyOnComplete: true,
    notifyOnError: true,
    downloadDir: 'C:\\Users\\melik\\Downloads\\Universal Downloader',
    defaultMode: 'video',
    defaultQuality: { type: 'best' },
    defaultContainer: 'mp4',
    maxConcurrentDownloads: 3,
    autoRetryCount: 2,
    filenameTemplate: '{creator} - {title}',
    theme: params.get('theme') ?? 'dark',
    language: params.get('lang') ?? 'tr',
    reduceMotion: false,
    showAnimatedBackground: true,
    lowResourceMode: false,
    hardwareAcceleration: true,
    cacheLimitMb: 256,
    ffmpegPath: null,
    enginePath: null,
    networkTimeoutSec: 30,
    proxyUrl: null,
    customUserAgent: null,
    debugLogging: false,
    browserLinkEnabled: true,
    hotkeys: {
      pasteUrl: 'Ctrl+V',
      download: 'Ctrl+Enter',
      openDownloads: 'Ctrl+J',
      openHistory: 'Ctrl+H',
      openSettings: 'Ctrl+,',
    },
    onboardingComplete: !params.has('welcome'),
  };

  const tool = (name, version) => ({
    name,
    available: true,
    path: `C:\\Users\\melik\\AppData\\Roaming\\UniversalDownloader\\tools\\${name}.exe`,
    version,
    source: 'managed',
  });
  const tools = {
    engine: tool('engine', '2026.09.12'),
    ffmpeg: tool('ffmpeg', '7.1'),
    jsRuntime: tool('jsRuntime', '2.4.1'),
  };
  /** The FFmpeg build `check_tool_update` reports as newer than the one above. */
  const FRESH_FFMPEG = 'N-126767-g7499a8ba58-20260922';

  const progress = (over = {}) => ({
    receivedBytes: 0,
    totalBytes: null,
    percent: null,
    speedBps: 0,
    etaSec: null,
    stage: 'waiting',
    stageIndex: 1,
    stageCount: 1,
    resumable: true,
    ...over,
  });

  const request = (url, platformId) => ({
    url,
    mode: 'video',
    quality: { type: 'best' },
    videoFormatId: null,
    audioFormatId: null,
    container: 'mp4',
    watermark: 'any',
    outputDir: null,
    title: null,
    thumbnailUrl: null,
    platform: platformId,
  });

  let seq = 0;
  const task = (platformId, title, status, over = {}) => {
    seq += 1;
    const url = `https://example.com/${platformId}/${seq}`;
    return {
      id: `task-${seq}`,
      url,
      title,
      platform: platformId,
      // Odd rows have a picture, even ones fall back to the platform tile.
      thumbnailUrl: seq % 2 ? `thumb:${seq}` : null,
      status,
      progress: progress(status === 'completed' ? { percent: 100, stage: 'done' } : {}),
      formatLabel: '1080p - MP4',
      outputPath:
        status === 'completed' ? `${settings.downloadDir}\\${title.slice(0, 40)}.mp4` : null,
      error: null,
      createdAt: now - seq * 7 * 60 * 1000,
      startedAt: now - seq * 7 * 60 * 1000,
      completedAt: status === 'completed' ? now - seq * 6 * 60 * 1000 : null,
      attempt: 1,
      request: request(url, platformId),
      ...over,
    };
  };

  let tasks = empty
    ? []
    : [
        task('youtube', 'Sıfır Bir - Abilerin Konuşurken Lafa Girme!', 'downloading', {
          progress: progress({
            receivedBytes: 48_300_000,
            totalBytes: 115_000_000,
            percent: 42,
            speedBps: 3_400_000,
            etaSec: 20,
            stage: 'video',
            stageCount: 2,
          }),
        }),
        task('tiktok', 'how it feels to cook as a Tom pearl / trend animation #frp', 'queued'),
        task('instagram', 'Photo by havadisamsun', 'completed', { formatLabel: 'Original - JPG' }),
        task('tiktok', 'What is he trying to do? || Cody Millers Prime 2020 #fyp #2020', 'completed'),
        task('twitter', 'Nerde Bu İnsanlar ? #discord #okul #okuladönüş', 'failed', {
          error: {
            code: 'network',
            title: 'Bağlantı kurulamadı',
            message: 'Sunucuya ulaşılamadı.',
            technical: 'HTTP 503 from cdn.example.com after 3 attempts',
            retryable: true,
          },
        }),
        task('reddit', 'bu şirketin oyunlarına gelmeyin!!! #kick #yayın', 'paused', {
          progress: progress({
            receivedBytes: 12_000_000,
            totalBytes: 80_000_000,
            percent: 15,
            stage: 'video',
          }),
        }),
        task('soundcloud', 'Late night mix vol. 4', 'completed', { formatLabel: '320 kbps - MP3' }),
        task('vimeo', 'A short film about nothing in particular', 'canceled'),
      ];

  const history = empty
    ? []
    : tasks
        .filter((entry) => entry.status === 'completed' || entry.status === 'failed')
        .map((entry, index) => ({
          id: index + 1,
          url: entry.url,
          title: entry.title,
          platform: entry.platform,
          thumbnailUrl: entry.thumbnailUrl,
          filePath: entry.outputPath ?? `${settings.downloadDir}\\missing.mp4`,
          fileExists: entry.status === 'completed' && index !== 2,
          container: 'mp4',
          qualityLabel: '1080p',
          fileSize: 24_000_000 + index * 9_100_000,
          createdAt: entry.createdAt,
          status: entry.status,
          request: entry.request,
        }));

  const conversions = empty
    ? []
    : [
        {
          id: 'convert-1',
          inputPath: 'C:\\Videos\\holiday.mov',
          inputName: 'holiday.mov',
          inputSizeBytes: 412_000_000,
          outputPath: null,
          outputSizeBytes: null,
          kind: 'video',
          status: 'running',
          percent: 63,
          durationSec: 184,
          streamCopied: false,
          error: null,
          createdAt: now - 60_000,
          startedAt: now - 50_000,
          completedAt: null,
          options: {
            targetFormat: 'mp4',
            outputDir: null,
            quality: 'balanced',
            maxHeight: null,
            audioBitrateKbps: null,
            allowStreamCopy: true,
          },
        },
        {
          id: 'convert-2',
          inputPath: 'C:\\Music\\interview.wav',
          inputName: 'interview.wav',
          inputSizeBytes: 96_000_000,
          outputPath: 'C:\\Music\\interview.mp3',
          outputSizeBytes: 8_700_000,
          kind: 'audio',
          status: 'completed',
          percent: 100,
          durationSec: 1260,
          streamCopied: false,
          error: null,
          createdAt: now - 3_600_000,
          startedAt: now - 3_590_000,
          completedAt: now - 3_500_000,
          options: {
            targetFormat: 'mp3',
            outputDir: null,
            quality: 'high',
            maxHeight: null,
            audioBitrateKbps: 192,
            allowStreamCopy: true,
          },
        },
      ];

  const format = (id, height, size) => ({
    id,
    kind: 'muxed',
    container: 'mp4',
    protocol: 'https',
    hasVideo: true,
    hasAudio: true,
    width: Math.round((height * 16) / 9),
    height,
    fps: 30,
    vcodec: 'avc1',
    acodec: 'mp4a',
    tbr: 2500,
    vbr: 2300,
    abr: 128,
    filesize: size,
    filesizeApprox: null,
    qualityLabel: `${height}p`,
    watermarked: null,
    note: null,
    needsEngineDownload: false,
  });

  const metadataFor = (url) => ({
    url,
    canonicalUrl: url,
    platform: detect(url),
    platformLabel: 'YouTube',
    providerId: 'engine',
    mediaKind: 'video',
    title: 'Big Buck Bunny — 60 fps, remastered',
    creator: 'Blender Foundation',
    description: null,
    thumbnailUrl: 'thumb:99',
    durationSec: 634,
    viewCount: 1_204_332,
    likeCount: 40_211,
    uploadDate: '20240312',
    isLive: false,
    formats: [format('22', 720, 48_000_000), format('37', 1080, 115_000_000)],
    entryCount: null,
    watermarkSupport: 'notApplicable',
    // Computed by Rust from the protocol of the streams it would fetch. Driven
    // off the host here so both branches of the link sheet can be looked at:
    // add "#whole" to a link to see the source that cannot hand over a range.
    rangeFetchable: !url.includes('#whole'),
    warnings: [],
  });

  function detect(url) {
    const hosts = [
      ['youtu', 'youtube'],
      ['tiktok', 'tiktok'],
      ['instagram', 'instagram'],
      ['twitter', 'twitter'],
      ['x.com', 'twitter'],
      ['reddit', 'reddit'],
      ['facebook', 'facebook'],
      ['twitch', 'twitch'],
      ['pinterest', 'pinterest'],
      ['vimeo', 'vimeo'],
      ['dailymotion', 'dailymotion'],
      ['soundcloud', 'soundcloud'],
    ];
    const match = hosts.find(([needle]) => url.includes(needle));
    if (match) return match[1];
    return /^https?:\/\//.test(url) ? 'generic' : 'unknown';
  }

  /** A picture that needs no network: two soft bands, different per id. */
  function thumbnail(key) {
    const hue = (Number(key.replace(/\D/g, '')) * 47) % 360;
    const svg =
      `<svg xmlns='http://www.w3.org/2000/svg' width='320' height='180'>` +
      `<defs><linearGradient id='g' x1='0' y1='0' x2='1' y2='1'>` +
      `<stop offset='0' stop-color='hsl(${hue} 55% 55%)'/>` +
      `<stop offset='1' stop-color='hsl(${(hue + 50) % 360} 60% 30%)'/>` +
      `</linearGradient></defs><rect width='320' height='180' fill='url(#g)'/></svg>`;
    return `data:image/svg+xml;utf8,${encodeURIComponent(svg)}`;
  }

  const update = {
    commit: 'f00dfeedf00dfeedf00dfeedf00dfeedf00dfeed',
    assetUrl:
      'https://github.com/unsalable/downloader/releases/download/latest/UniversalDownloader_x64-setup.exe',
    assetSize: 3_772_494,
    digest: 'sha256:ec068ea0bf5a95ab1fecdd560395d22ddb6295e3cf43f1b3b6894aafd6992adc',
    publishedAt: new Date(now - 3_600_000).toISOString(),
  };

  // -- events ----------------------------------------------------------------

  const listeners = new Map();
  let listenerSeq = 0;

  /**
   * A real, very small H.264 clip: 20.4s of test pattern at 160x90. The
   * Trim screen is mostly a video element with marks under it, and a
   * preview with an empty <video> would not show whether any of it lines
   * up. Inline because this file is injected on its own, with nothing
   * beside it to fetch.
   */
  const PREVIEW_CLIP = 'data:video/mp4;base64,AAAAIGZ0eXBpc29tAAACAGlzb21pc28yYXZjMW1wNDEAAAyxbW9vdgAAAGxtdmhkAAAAAAAAAAAAAAAAAAAD6AAATiAAAQAAAQAAAAAAAAAAAAAAAAEAAAAAAAAAAAAAAAAAAAABAAAAAAAAAAAAAAAAAABAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAgAAC9x0cmFrAAAAXHRraGQAAAADAAAAAAAAAAAAAAABAAAAAAAATiAAAAAAAAAAAAAAAAAAAAAAAAEAAAAAAAAAAAAAAAAAAAABAAAAAAAAAAAAAAAAAABAAAAAAKAAAABaAAAAAAAkZWR0cwAAABxlbHN0AAAAAAAAAAEAAE4gAAAIAAABAAAAAAtUbWRpYQAAACBtZGhkAAAAAAAAAAAAAAAAAAAwAAADwABVxAAAAAAALWhkbHIAAAAAAAAAAHZpZGUAAAAAAAAAAAAAAABWaWRlb0hhbmRsZXIAAAAK/21pbmYAAAAUdm1oZAAAAAEAAAAAAAAAAAAAACRkaW5mAAAAHGRyZWYAAAAAAAAAAQAAAAx1cmwgAAAAAQAACr9zdGJsAAAAw3N0c2QAAAAAAAAAAQAAALNhdmMxAAAAAAAAAAEAAAAAAAAAAAAAAAAAAAAAAKAAWgBIAAAASAAAAAAAAAABFUxhdmM2My4xMy4xMDAgbGlieDI2NAAAAAAAAAAAAAAAGP//AAAAOWF2Y0MBZAAM/+EAG2dkAAyscgRCjfkwEQAAAwABAAADABgPFCmEYAEAB2joQ4OSyLD9+PgAAAAAEHBhc3AAAAABAAAAAQAAABRidHJ0AAAAAAAAHzIAAAAAAAAAGHN0dHMAAAAAAAAAAQAAAPAAAAQAAAAAOHN0c3MAAAAAAAAACgAAAAEAAAAZAAAAMQAAAEkAAABhAAAAeQAAAJEAAACpAAAAwQAAANkAAAWgY3R0cwAAAAAAAACyAAAAAQAACAAAAAABAAAMAAAAAAEAAAQAAAAAAQAAFAAAAAABAAAIAAAAAAEAAAAAAAAAAQAABAAAAAABAAAQAAAAAAIAAAQAAAAAAQAAGAAAAAABAAAIAAAAAAEAAAAAAAAAAgAABAAAAAABAAAkAAAAAAEAABAAAAAAAwAAAAAAAAADAAAEAAAAAAIAAAgAAAAAAQAAEAAAAAACAAAEAAAAAAEAABgAAAAAAQAACAAAAAABAAAAAAAAAAIAAAQAAAAAAQAAGAAAAAABAAAIAAAAAAEAAAAAAAAAAgAABAAAAAABAAAcAAAAAAEAAAwAAAAAAgAAAAAAAAACAAAEAAAAAAEAABQAAAAAAQAACAAAAAABAAAAAAAAAAEAAAQAAAAAAQAACAAAAAABAAAYAAAAAAEAAAgAAAAAAQAAAAAAAAACAAAEAAAAAAEAABgAAAAAAQAACAAAAAABAAAAAAAAAAIAAAQAAAAAAQAAGAAAAAABAAAIAAAAAAEAAAAAAAAAAgAABAAAAAABAAAgAAAAAAEAAAwAAAAAAgAAAAAAAAADAAAEAAAAAAIAAAgAAAAAAQAADAAAAAABAAAEAAAAAAEAABQAAAAAAQAACAAAAAABAAAAAAAAAAEAAAQAAAAAAQAAEAAAAAACAAAEAAAAAAEAABgAAAAAAQAACAAAAAABAAAAAAAAAAIAAAQAAAAAAQAAJAAAAAABAAAQAAAAAAMAAAAAAAAAAwAABAAAAAACAAAIAAAAAAEAABAAAAAAAgAABAAAAAABAAAYAAAAAAEAAAgAAAAAAQAAAAAAAAACAAAEAAAAAAEAABgAAAAAAQAACAAAAAABAAAAAAAAAAIAAAQAAAAAAQAAHAAAAAABAAAMAAAAAAIAAAAAAAAAAgAABAAAAAABAAAUAAAAAAEAAAgAAAAAAQAAAAAAAAABAAAEAAAAAAEAAAgAAAAAAQAAGAAAAAABAAAIAAAAAAEAAAAAAAAAAgAABAAAAAABAAAYAAAAAAEAAAgAAAAAAQAAAAAAAAACAAAEAAAAAAEAABgAAAAAAQAACAAAAAABAAAAAAAAAAIAAAQAAAAAAQAAIAAAAAABAAAMAAAAAAIAAAAAAAAAAwAABAAAAAACAAAIAAAAAAEAAAwAAAAAAQAABAAAAAABAAAUAAAAAAEAAAgAAAAAAQAAAAAAAAABAAAEAAAAAAEAABAAAAAAAgAABAAAAAABAAAYAAAAAAEAAAgAAAAAAQAAAAAAAAACAAAEAAAAAAEAACQAAAAAAQAAEAAAAAADAAAAAAAAAAMAAAQAAAAAAgAACAAAAAABAAAQAAAAAAIAAAQAAAAAAQAAGAAAAAABAAAIAAAAAAEAAAAAAAAAAgAABAAAAAABAAAYAAAAAAEAAAgAAAAAAQAAAAAAAAAC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';

  /** The editor's three managers, each with its own fake progress. */
  const exporting = { status: 'idle', percent: null, outputPath: null, error: null, timer: null };

  /** The app's own folder, where a link fetched into the editor lands. */
  const APP_TEMP = 'C:\\Users\\melik\\AppData\\Roaming\\UniversalDownloader\\temp\\';

  /**
   * Where an export goes when no folder is chosen, if not beside the source:
   * the downloads folder, for every file on a phone and for the app's own
   * files on the desktop -- as `ExportManager::default_dir` decides.
   */
  const exportDefaultDir = (path) =>
    platform === 'android' || /[\\/]AppData[\\/]Roaming[\\/]UniversalDownloader[\\/]/i.test(path)
      ? settings.downloadDir
      : null;

  /** Paths that sound like sound, which the probe reports without a picture. */
  const AUDIO_ONLY = /\.(mp3|m4a|aac|opus|ogg|flac|wav)$/i;

  /** What the phone's file picker returns (see platform_pick_media_files). */
  const picker = {
    picked: ['/data/user/0/io.universaldownloader.app/cache/imports/tatil klibi.mp4'],
  };
  const fetching = {
    status: 'idle',
    percent: null,
    receivedBytes: 0,
    title: null,
    outputPath: null,
    error: null,
    timer: null,
  };

  /**
   * A waveform that looks like music rather than like noise: a slow swell with
   * a beat on it, so the peaks the editor draws have a shape a person would
   * recognise and the drawing code is exercised at both extremes.
   */
  const wavePeaks = (buckets) => {
    const bytes = new Uint8Array(buckets * 2);
    for (let i = 0; i < buckets; i += 1) {
      const swell = 0.12 + 0.5 * Math.sin((i / buckets) * Math.PI * 2.5) ** 2;
      const beat = i % Math.round(buckets / 64) < 3 ? 1.6 : 0.4;
      const amplitude = Math.min(1, swell * beat);
      bytes[i * 2] = Math.round(128 - amplitude * 127);
      bytes[i * 2 + 1] = Math.round(128 + amplitude * 127);
    }
    let binary = '';
    for (const byte of bytes) binary += String.fromCharCode(byte);
    return btoa(binary);
  };

  /**
   * A filmstrip chunk, drawn rather than embedded: eight cells of shifting hue
   * with their index on them, which is enough to see whether the strip is laid
   * out in the right order and at the right scale.
   */
  const stripChunk = (index, cells, cellWidth, cellHeight) => {
    const canvas = document.createElement('canvas');
    canvas.width = cells * cellWidth;
    canvas.height = cellHeight;
    const ctx = canvas.getContext('2d');
    for (let i = 0; i < cells; i += 1) {
      const hue = ((index * cells + i) * 9) % 360;
      ctx.fillStyle = `hsl(${hue} 45% ${28 + (i % 2) * 8}%)`;
      ctx.fillRect(i * cellWidth, 0, cellWidth, cellHeight);
      ctx.fillStyle = 'rgba(255,255,255,0.55)';
      ctx.font = '11px sans-serif';
      ctx.fillText(String(index * cells + i), i * cellWidth + 4, cellHeight - 6);
    }
    return canvas.toDataURL('image/jpeg', 0.7);
  };

  // One timer per kind: the two requests arrive together, and a single timer
  // meant the filmstrip's cancelled the waveform's before it ever emitted.
  const timeline = { path: null, token: 0, working: false, waveform: null, filmstrip: null, error: null, timers: {} };
  const publishExport = (over) => {
    Object.assign(exporting, over);
    const { timer, ...state } = exporting;
    return state;
  };

  const publishFetch = (over) => {
    Object.assign(fetching, over);
    const { timer, ...state } = fetching;
    return state;
  };

  const publishTimeline = (over) => {
    Object.assign(timeline, over);
    const { timers, ...state } = timeline;
    return state;
  };

  function emit(event, payload) {
    for (const entry of listeners.values()) {
      if (entry.event === event) window[`_${entry.handler}`]?.({ event, id: entry.id, payload });
    }
  }

  const changed = () => emit('download://changed', tasks);
  const patch = (id, over) => {
    tasks = tasks.map((entry) => (entry.id === id ? { ...entry, ...over } : entry));
    changed();
  };

  // -- commands --------------------------------------------------------------

  const wait = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
  const calls = [];

  const commands = {
    get_settings: () => settings,
    save_settings: ({ settings: next }) => Object.assign(settings, next),
    reset_settings: () => settings,

    get_tools: () => tools,
    refresh_tools: () => tools,
    // Walks through the stages the real install reports, and leaves FFmpeg at
    // the build `check_tool_update` says is newer.
    install_tool: async ({ tool }) => {
      const total = 86_326_164;
      for (let step = 1; step <= 12; step += 1) {
        await wait(150);
        emit('tools://progress', {
          tool,
          receivedBytes: (total * step) / 12,
          totalBytes: total,
          stage: 'downloading',
        });
      }
      emit('tools://progress', { tool, receivedBytes: 0, totalBytes: null, stage: 'extracting' });
      await wait(600);
      emit('tools://progress', { tool, receivedBytes: 0, totalBytes: null, stage: 'verifying' });
      await wait(400);
      if (tool === 'ffmpeg') tools.ffmpeg = { ...tools.ffmpeg, version: FRESH_FFMPEG };
      emit('tools://changed', tools);
      return tools;
    },
    // The engine answers current and FFmpeg that a newer build is up, so both
    // ways "check for update" can go are on screen. Once installed, FFmpeg
    // answers current too.
    check_tool_update: async ({ tool }) => {
      await wait(700);
      const installed = tools[tool].version;
      if (tool === 'ffmpeg') {
        return { tool, installed, latest: '2026-09-22', upToDate: installed === FRESH_FFMPEG };
      }
      return { tool, installed, latest: installed, upToDate: true };
    },

    bridge_status: () => ({
      supported: platform === 'windows',
      storeListed: true,
      enabled: true,
      registered: true,
      connected: true,
      browser: 'Chrome',
      profileLabel: 'Melih',
      accountHint: 'm•••@gmail.com',
      extensionVersion: '1.0.0',
      lastPushAt: Math.floor(now / 1000) - 600,
      session: 'fresh',
      hostPath: 'C:\\Users\\melik\\AppData\\Local\\Universal Downloader\\ud-bridge.exe',
      appVersion: '1.0.0',
      extensionId: 'abcdefghijklmnopabcdefghijklmnop',
    }),
    bridge_repair: () => commands.bridge_status(),
    bridge_disconnect: () => commands.bridge_status(),
    bridge_diagnostics: () => 'bridge: ok',

    detect_platform: ({ url }) => detect(url),
    analyze_url: async ({ url }) => {
      await wait(900);
      if (url.includes('fail')) {
        throw {
          code: 'unsupported',
          title: 'Bu bağlantı desteklenmiyor',
          message: 'Bu sayfada indirilebilir bir medya bulunamadı.',
          technical: 'ERROR: Unsupported URL',
          retryable: true,
        };
      }
      return metadataFor(url);
    },
    get_thumbnail: ({ url }) => {
      if (!url.startsWith('thumb:')) throw new Error('no thumbnail');
      return thumbnail(url);
    },
    summarize_plan: () => ({
      label: '1080p - MP4',
      qualityLabel: '1080p',
      container: 'mp4',
      needsMerge: false,
      needsFfmpeg: false,
      estimatedBytes: 115_000_000,
      videoFormatId: '37',
      audioFormatId: null,
      stageCount: 1,
    }),

    list_downloads: () => tasks,
    enqueue_download: ({ request: asked }) => {
      const added = task(asked.platform ?? 'generic', asked.title ?? asked.url, 'downloading', {
        createdAt: Date.now(),
        thumbnailUrl: asked.thumbnailUrl,
        progress: progress({ totalBytes: 115_000_000, percent: 0, stage: 'video' }),
      });
      tasks = [...tasks, added];
      changed();
      return added;
    },
    enqueue_gallery: (args) => [commands.enqueue_download(args)],
    pause_download: ({ id }) => patch(id, { status: 'paused' }),
    resume_download: ({ id }) => patch(id, { status: 'downloading' }),
    cancel_download: ({ id }) => patch(id, { status: 'canceled' }),
    retry_download: ({ id }) => patch(id, { status: 'queued', error: null }),
    remove_download: ({ id }) => {
      tasks = tasks.filter((entry) => entry.id !== id);
      changed();
    },
    pause_all_downloads: () => {
      tasks = tasks.map((entry) =>
        entry.status === 'downloading' ? { ...entry, status: 'paused' } : entry,
      );
      changed();
    },
    resume_all_downloads: () => {
      tasks = tasks.map((entry) =>
        entry.status === 'paused' ? { ...entry, status: 'downloading' } : entry,
      );
      changed();
    },
    clear_finished_downloads: () => {
      tasks = tasks.filter((entry) => !['completed', 'failed', 'canceled'].includes(entry.status));
      changed();
    },
    reorder_download: () => null,
    set_download_order: () => null,

    convert_formats: () => [
      { id: 'mp4', kind: 'video' },
      { id: 'mkv', kind: 'video' },
      { id: 'webm', kind: 'video' },
      { id: 'mp3', kind: 'audio' },
      { id: 'm4a', kind: 'audio' },
      { id: 'wav', kind: 'audio' },
    ],
    probe_media: ({ path }) => {
      // A file named like sound is sound: no picture, which is what the editor
      // has to turn away (the phone's picker offers audio as well).
      const sound = AUDIO_ONLY.test(path);
      return {
        path,
        fileName: path.split(/[\\/]/).pop(),
        container: sound ? 'mp3' : 'mov',
        // About 8.4 Mbps over the 20 seconds below, which is what a 1080p H.264
        // file really runs at -- the custom bitrate is prefilled from it.
        sizeBytes: 21_000_000,
        // Exactly the length of PREVIEW_CLIP below, so the timeline and the
        // picture above it are describing the same thing. It was 20.4 here for a
        // while, which put the end mark a little past the end of the video.
        durationSec: 20,
        videoDurationSec: sound ? null : 20,
        width: sound ? null : 1920,
        height: sound ? null : 1080,
        fps: sound ? null : 30,
        videoCodec: sound ? null : 'h264',
        audioCodec: 'aac',
        audioBitrateKbps: 192,
        hasVideo: !sound,
        hasAudio: true,
      };
    },
    list_conversions: () => conversions,
    enqueue_conversions: () => conversions,
    cancel_conversion: () => null,
    retry_conversion: () => null,
    remove_conversion: () => null,
    clear_finished_conversions: () => null,

    allow_media_preview: () => null,

    export_state: () => publishExport({}),
    cancel_export: () => {
      clearInterval(exporting.timer);
      exporting.timer = null;
      emit('editor://export', publishExport({ status: 'canceled', percent: null, outputPath: null }));
      return null;
    },
    start_export: ({ request }) => {
      clearInterval(exporting.timer);
      emit('editor://export', publishExport({ status: 'running', percent: 0, outputPath: null, error: null }));
      let percent = 0;
      exporting.timer = setInterval(() => {
        percent += 9;
        if (percent < 100) {
          emit('editor://export', publishExport({ status: 'running', percent }));
          return;
        }
        clearInterval(exporting.timer);
        exporting.timer = null;
        // The range the app wrote into a name is replaced, not added to.
        const name = request.inputPath
          .split(/[\\/]/)
          .pop()
          .replace(/\.[^.]+$/, '')
          .replace(/ \d+m\d{2}(?:\.\d)?s-\d+m\d{2}(?:\.\d)?s(?: \(\d+\))?$/, '');
        // A phone's exports land in its downloads, where the gallery finds them;
        // a desktop's where it was told, or beside the source unless that is
        // one of the app's own folders.
        const folder =
          platform === 'android'
            ? '/storage/emulated/0/Download/Universal Downloader/'
            : (request.outputDir ??
                exportDefaultDir(request.inputPath) ??
                request.inputPath.replace(/[^\\/]+$/, '')
              ).replace(/[\\/]*$/, '\\');
        emit('editor://export', publishExport({
          status: 'completed',
          percent: 100,
          outputPath: folder + name + ' 0m04s-0m15.4s.' + request.options.container,
        }));
      }, 220);
      return null;
    },

    // Every two seconds, which is what a long file's keyframes really look
    // like -- far enough apart that a lossless cut visibly lands off the mark.
    media_keyframes: () => Array.from({ length: 11 }, (_, i) => i * 2),
    export_default_dir: ({ path }) => exportDefaultDir(path),

    fetch_state: () => publishFetch({}),
    cancel_range_fetch: () => {
      clearInterval(fetching.timer);
      fetching.timer = null;
      emit('editor://fetch', publishFetch({ status: 'canceled', percent: null }));
      return null;
    },
    start_range_fetch: ({ request }) => {
      clearInterval(fetching.timer);
      // Resolving reports nothing, exactly as the engine does not: the sheet
      // has to be able to show an honest wait rather than a bar at zero.
      emit('editor://fetch', publishFetch({
        status: 'resolving',
        percent: null,
        receivedBytes: 0,
        title: 'Big Buck Bunny',
        outputPath: null,
        error: null,
      }));
      let ticks = 0;
      fetching.timer = setInterval(() => {
        ticks += 1;
        if (ticks < 4) {
          emit('editor://fetch', publishFetch({ status: 'fetching', percent: null, receivedBytes: ticks * 180_000 }));
          return;
        }
        clearInterval(fetching.timer);
        fetching.timer = null;
        const ranged = request.startSec != null;
        emit('editor://fetch', publishFetch({
          status: 'completed',
          percent: 100,
          // The app's own temporary folder unless a folder was asked for, as
          // `range::destination_dir` decides.
          outputPath:
            (request.outputDir ? request.outputDir.replace(/[\\/]*$/, '\\') : APP_TEMP) +
            'Big Buck Bunny' + (ranged ? ' 0m10s-0m25s' : '') + '.mp4',
        }));
      }, 420);
      return null;
    },

    timeline_state: () => publishTimeline({}),
    cancel_timeline: () => {
      for (const timer of Object.values(timeline.timers)) clearTimeout(timer);
      timeline.timers = {};
      return null;
    },
    request_timeline: ({ request }) => {
      clearTimeout(timeline.timers[request.kind]);
      emit('editor://timeline', publishTimeline({ path: request.path, token: request.token, working: true }));
      const start = request.startSec ?? 0;
      const length = request.lengthSec ?? 20;
      timeline.timers[request.kind] = setTimeout(() => {
        if (request.kind === 'waveform') {
          emit('editor://timeline', publishTimeline({
            working: false,
            waveform: {
              buckets: request.count,
              startSec: start,
              lengthSec: length,
              peaks: wavePeaks(request.count),
            },
          }));
          return;
        }
        const chunkFrames = 8;
        const chunks = Math.max(1, Math.round(request.count / chunkFrames));
        const cellHeight = request.cellHeight ?? 68;
        const cellWidth = Math.round((cellHeight * 16) / 9);
        emit('editor://timeline', publishTimeline({
          working: false,
          filmstrip: {
            frames: chunks * chunkFrames,
            chunkFrames,
            cellWidth,
            cellHeight,
            startSec: start,
            lengthSec: length,
            chunks: Array.from({ length: chunks }, (_, i) => stripChunk(i, chunkFrames, cellWidth, cellHeight)),
          },
        }));
      }, 220);
      return null;
    },

    // One frame, for the rail's thumbnails and for files the window cannot
    // decode. A single cell of the same drawn strip is close enough.
    frame_at: ({ height }) => stripChunk(0, 1, Math.round(((height ?? 68) * 16) / 9), height ?? 68),

    list_history: ({ query }) =>
      query ? history.filter((entry) => entry.title.toLowerCase().includes(query.toLowerCase())) : history,
    count_history: () => history.length,
    delete_history_entry: () => null,
    clear_history: () => null,

    path_exists: () => true,
    preview_filename: ({ template }) => `${template.replace(/[{}]/g, '')}.mp4`,
    cache_stats: () => ({
      thumbnailCount: 84,
      thumbnailBytes: 6_400_000,
      metadataCount: 31,
      totalBytes: 7_100_000,
    }),
    clear_cache: () => null,
    get_diagnostics: () => ({
      appVersion: '1.0.0',
      os: 'Windows 11 (26200)',
      engine: tools.engine,
      ffmpeg: tools.ffmpeg,
      downloadDir: settings.downloadDir,
      dbPath: 'C:\\Users\\melik\\AppData\\Roaming\\UniversalDownloader\\app.db',
      logPath: 'C:\\Users\\melik\\AppData\\Roaming\\UniversalDownloader\\logs\\app.log',
      activeDownloads: 1,
      queuedDownloads: 1,
    }),
    get_log_dir: () => 'C:\\Users\\melik\\AppData\\Roaming\\UniversalDownloader\\logs',
    get_licenses: () => ({
      packages: [
        { name: 'react', version: '19.2.8', license: 'MIT', kind: 'npm', url: 'https://react.dev' },
        { name: 'tauri', version: '2.11.5', license: 'MIT OR Apache-2.0', kind: 'cargo', url: 'https://tauri.app' },
        { name: 'Inter', version: '4.1', license: 'OFL-1.1', kind: 'font', url: 'https://rsms.me/inter' },
      ],
    }),
    get_app_version: () => '1.0.0',
    get_build_commit: () => '05b60a1c2b1b5d97eaf1e60eda459987c70f717b',
    sweep_temp_files: () => 0,

    platform_open_file: () => null,
    platform_open_downloads: () => null,
    platform_open_app_settings: () => null,
    // What the Android picker hands back once it has copied the choice into
    // the app's cache. Set `__UD_MOCK__.picked` from the console to pick
    // something else -- a name ending in .mp3 is a file with no picture.
    platform_pick_media_files: () => [...picker.picked],
    platform_set_system_bars: () => null,
    platform_take_shared_text: () => null,

    check_app_update: () => (params.has('update') ? update : null),
    install_app_update: async () => {
      for (let step = 1; step <= 10; step += 1) {
        await wait(150);
        emit('update://progress', {
          receivedBytes: (update.assetSize * step) / 10,
          totalBytes: update.assetSize,
        });
      }
    },
    // The real command never returns: the app exits under it.
    apply_app_update: () => new Promise(() => {}),
  };

  async function invoke(cmd, args = {}) {
    calls.push({ cmd, args });

    if (cmd === 'plugin:event|listen') {
      listenerSeq += 1;
      listeners.set(listenerSeq, { id: listenerSeq, event: args.event, handler: args.handler });
      return listenerSeq;
    }
    if (cmd === 'plugin:event|unlisten') {
      listeners.delete(args.eventId);
      return null;
    }
    if (cmd === 'plugin:event|emit' || cmd === 'plugin:event|emit_to') {
      emit(args.event, args.payload);
      return null;
    }
    // The file and folder pickers. Without these the editor's "choose a file"
    // button does nothing at all in the harness, which reads as a broken
    // screen rather than as a missing fixture.
    if (cmd === 'plugin:dialog|open') {
      const options = args.options ?? {};
      if (options.directory) return 'C:\\Users\\melik\\Videos\\Kesimler';
      const picked = ['C:\\Users\\melik\\Videos\\tatil klibi.mp4'];
      if (options.multiple) return picked;
      return picked[0];
    }
    if (cmd === 'plugin:clipboard-manager|read_text') return '';
    if (cmd === 'plugin:autostart|is_enabled') return false;
    if (cmd === 'plugin:app|version') return '1.0.0';
    if (cmd === 'plugin:notification|is_permission_granted') return true;
    if (cmd.startsWith('plugin:')) return null;

    const handler = commands[cmd];
    if (!handler) {
      console.warn(`[mock-tauri] unhandled command: ${cmd}`, args);
      return null;
    }
    return handler(args);
  }

  let callbackSeq = 0;
  window.__TAURI_INTERNALS__ = {
    invoke,
    transformCallback(callback, once = false) {
      callbackSeq += 1;
      const id = callbackSeq;
      const name = `_${id}`;
      Object.defineProperty(window, name, {
        configurable: true,
        value: (result) => {
          if (once) Reflect.deleteProperty(window, name);
          return callback?.(result);
        },
      });
      return id;
    },
    unregisterCallback(id) {
      Reflect.deleteProperty(window, `_${id}`);
    },
    // A real path means nothing to a browser, so the Trim screen gets the
    // clip embedded above -- which is what makes its <video> show anything.
    convertFileSrc: () => PREVIEW_CLIP,
    metadata: {
      currentWindow: { label: 'main' },
      currentWebview: { label: 'main', windowLabel: 'main' },
    },
  };
  window.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener: () => {} };
  window.__TAURI_OS_PLUGIN_INTERNALS__ = {
    platform,
    os_type: platform,
    family: platform === 'windows' ? 'windows' : 'unix',
    version: '10.0.26200',
    arch: 'x86_64',
    eol: '\r\n',
    exe_extension: platform === 'windows' ? 'exe' : '',
  };

  // The running download keeps moving, so progress rendering can be watched.
  setInterval(() => {
    for (const entry of tasks) {
      if (entry.status !== 'downloading') continue;
      const total = entry.progress.totalBytes ?? 100_000_000;
      const received = Math.min(total, entry.progress.receivedBytes + 1_700_000);
      const done = received >= total;
      const next = {
        ...entry.progress,
        receivedBytes: received,
        totalBytes: total,
        percent: (received / total) * 100,
        speedBps: done ? 0 : 3_100_000 + Math.round(Math.random() * 600_000),
        etaSec: done ? 0 : Math.round((total - received) / 3_400_000),
        stage: done ? 'done' : 'video',
      };
      const status = done ? 'completed' : 'downloading';
      const outputPath = done ? `${settings.downloadDir}\\${entry.title.slice(0, 40)}.mp4` : null;
      tasks = tasks.map((other) =>
        other.id === entry.id
          ? { ...other, status, progress: next, outputPath, completedAt: done ? Date.now() : null }
          : other,
      );
      emit('download://progress', { id: entry.id, status, progress: next, outputPath, error: null });
    }
  }, 500);

  window.__UD_MOCK__ = {
    emit,
    calls,
    settings,
    get tasks() { return tasks; },
    get picked() { return picker.picked; },
    set picked(paths) { picker.picked = paths; },
  };
})();

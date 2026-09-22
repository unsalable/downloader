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
    install_tool: () => tools,

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
    probe_media: ({ path }) => ({
      path,
      fileName: path.split(/[\\/]/).pop(),
      container: 'mov',
      sizeBytes: 412_000_000,
      durationSec: 184,
      width: 1920,
      height: 1080,
      fps: 30,
      videoCodec: 'h264',
      audioCodec: 'aac',
      audioBitrateKbps: 192,
      hasVideo: true,
      hasAudio: true,
    }),
    list_conversions: () => conversions,
    enqueue_conversions: () => conversions,
    cancel_conversion: () => null,
    retry_conversion: () => null,
    remove_conversion: () => null,
    clear_finished_conversions: () => null,

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
    platform_pick_media_files: () => [],
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
    convertFileSrc: (path) => path,
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

  window.__UD_MOCK__ = { emit, calls, settings, get tasks() { return tasks; } };
})();

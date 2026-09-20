import { AnimatePresence, motion } from 'motion/react';
import { readText } from '@tauri-apps/plugin-clipboard-manager';
import { Images, RotateCcw } from 'lucide-react';
import { useCallback, useEffect, useRef, useState } from 'react';

import { AnalyzingCard } from '@/components/home/AnalyzingCard';
import { DownloadOptionsPanel } from '@/components/home/DownloadOptionsPanel';
import { ErrorCard } from '@/components/home/ErrorCard';
import { Hero } from '@/components/home/Hero';
import { MediaPreviewCard } from '@/components/home/MediaPreviewCard';
import { PlatformIndicator } from '@/components/home/PlatformIndicator';
import { UrlInput, type UrlInputHandle } from '@/components/home/UrlInput';
import { InstallProgress } from '@/components/settings/ToolCard';
import { Button } from '@/components/ui/Button';
import { useDebouncedValue } from '@/hooks/useDebouncedValue';
import { useTranslation } from '@/i18n';
import { cn } from '@/lib/cn';
import { RISE, T } from '@/lib/motion';
import { IS_MOBILE } from '@/lib/platform';
import { normalizeUrl } from '@/lib/url';
import * as ipc from '@/services/ipc';
import { useAnalysisStore } from '@/stores/useAnalysisStore';
import { useToastStore } from '@/stores/useToastStore';
import { useToolsStore } from '@/stores/useToolsStore';
import type { DownloadRequest, Settings } from '@/types';

interface HomePageProps {
  settings: Settings;
  onGoToDownloads: () => void;
  onOpenSettings: () => void;
  inputRef: React.RefObject<UrlInputHandle | null>;
}

export function HomePage({
  settings,
  onGoToDownloads,
  onOpenSettings,
  inputRef,
}: HomePageProps) {
  const { t } = useTranslation();

  const url = useAnalysisStore((state) => state.url);
  const phase = useAnalysisStore((state) => state.phase);
  const platform = useAnalysisStore((state) => state.platform);
  const metadata = useAnalysisStore((state) => state.metadata);
  const error = useAnalysisStore((state) => state.error);
  const options = useAnalysisStore((state) => state.options);
  const setUrl = useAnalysisStore((state) => state.setUrl);
  const setPlatform = useAnalysisStore((state) => state.setPlatform);
  const setOptions = useAnalysisStore((state) => state.setOptions);
  const analyze = useAnalysisStore((state) => state.analyze);
  const reset = useAnalysisStore((state) => state.reset);

  const tools = useToolsStore((state) => state.tools);
  const checkingTools = useToolsStore((state) => state.checking);
  const engineInstall = useToolsStore((state) => state.installing.engine);
  const installTool = useToolsStore((state) => state.install);
  const pushToast = useToastStore((state) => state.push);

  const [submitting, setSubmitting] = useState(false);
  const [dragging, setDragging] = useState(false);

  const engineReady = tools?.engine.available ?? false;
  const engineMissing = !engineReady && tools != null && !checkingTools;

  // Platform detection is a pure function in Rust; debouncing keeps it off the
  // keystroke path without duplicating the host patterns in TypeScript.
  const debouncedUrl = useDebouncedValue(url, 140);
  useEffect(() => {
    let active = true;
    if (!debouncedUrl.trim()) {
      setPlatform('unknown');
      return;
    }
    ipc.detectPlatform(debouncedUrl).then((detected) => {
      if (active) setPlatform(detected);
    });
    return () => {
      active = false;
    };
  }, [debouncedUrl, setPlatform]);

  const startAnalysis = useCallback(
    (target: string) => {
      if (!engineReady) return;
      void analyze(target, {
        mode: settings.defaultMode,
        quality: settings.defaultQuality,
        container: settings.defaultContainer,
        outputDir: null,
      });
    },
    [analyze, engineReady, settings.defaultContainer, settings.defaultMode, settings.defaultQuality],
  );

  // An analysis that failed only because the engine was missing is retried the
  // moment the engine appears, wherever it was installed from. Otherwise the
  // error card would keep saying "missing" about a tool that is now there.
  const wasEngineReady = useRef(engineReady);
  useEffect(() => {
    const appeared = engineReady && !wasEngineReady.current;
    wasEngineReady.current = engineReady;
    if (!appeared) return;

    const current = useAnalysisStore.getState();
    if (current.phase === 'error' && current.error?.code === 'engineMissing' && current.url.trim()) {
      startAnalysis(current.url);
    }
  }, [engineReady, startAnalysis]);

  const pasteFromClipboard = useCallback(async () => {
    try {
      const text = await readText();
      const normalized = text ? normalizeUrl(text) : null;
      if (!normalized) return;
      setUrl(normalized);
      startAnalysis(normalized);
    } catch {
      // Clipboard can be unavailable; the field is still usable by typing.
    }
  }, [setUrl, startAnalysis]);

  // Drag and drop of a link or a text fragment containing one.
  const dropDepth = useRef(0);
  useEffect(() => {
    const onDragEnter = (event: DragEvent) => {
      event.preventDefault();
      dropDepth.current += 1;
      setDragging(true);
    };
    const onDragLeave = (event: DragEvent) => {
      event.preventDefault();
      dropDepth.current -= 1;
      if (dropDepth.current <= 0) {
        dropDepth.current = 0;
        setDragging(false);
      }
    };
    const onDragOver = (event: DragEvent) => event.preventDefault();
    const onDrop = (event: DragEvent) => {
      event.preventDefault();
      dropDepth.current = 0;
      setDragging(false);

      const text =
        event.dataTransfer?.getData('text/uri-list') || event.dataTransfer?.getData('text/plain');
      const normalized = text ? normalizeUrl(text.split('\n')[0] ?? '') : null;
      if (!normalized) return;

      setUrl(normalized);
      startAnalysis(normalized);
    };

    window.addEventListener('dragenter', onDragEnter);
    window.addEventListener('dragleave', onDragLeave);
    window.addEventListener('dragover', onDragOver);
    window.addEventListener('drop', onDrop);
    return () => {
      window.removeEventListener('dragenter', onDragEnter);
      window.removeEventListener('dragleave', onDragLeave);
      window.removeEventListener('dragover', onDragOver);
      window.removeEventListener('drop', onDrop);
    };
  }, [setUrl, startAnalysis]);

  const buildRequest = (): DownloadRequest | null => {
    if (!metadata) return null;
    return {
      url: metadata.canonicalUrl || metadata.url,
      mode: options.mode,
      quality: options.quality,
      videoFormatId: options.advanced ? options.videoFormatId : null,
      audioFormatId: options.advanced ? options.audioFormatId : null,
      container: options.container,
      watermark: options.watermark,
      outputDir: options.outputDir,
      title: metadata.title,
      thumbnailUrl: metadata.thumbnailUrl,
      platform: metadata.platform,
    };
  };

  const startDownload = async (asGallery: boolean) => {
    const request = buildRequest();
    if (!request) return;

    setSubmitting(true);
    try {
      // A carousel or album becomes one task per item, so each gets its own
      // progress, retry and history row.
      const count = asGallery ? (await ipc.enqueueGallery(request)).length : 1;
      if (!asGallery) await ipc.enqueueDownload(request);

      // On a phone the download itself is the next thing to look at, and the
      // screen that shows it is a tab away; going there says "added" on its own.
      if (IS_MOBILE) {
        reset();
        onGoToDownloads();
        return;
      }

      pushToast({
        tone: 'success',
        title: t('action.addedToQueue'),
        body: count > 1 ? t('preview.entries', { n: count }) : metadata?.title,
        actions: [{ label: t('nav.downloads'), onClick: onGoToDownloads, primary: true }],
      });
      reset();
    } catch (caught) {
      const info = ipc.toAppError(caught);
      pushToast({ tone: 'error', title: info.title, body: info.message, durationMs: 7000 });
    } finally {
      setSubmitting(false);
    }
  };

  const installEngine = async () => {
    const ok = await installTool('engine');
    pushToast(
      ok
        ? { tone: 'success', title: t('settings.engine'), body: t('common.done') }
        : {
            tone: 'error',
            title: t('settings.toolInstallFailed'),
            body: useToolsStore.getState().error ?? t('error.network.message'),
            durationMs: 9000,
          },
    );
  };

  // What the user can do about a failure, beyond trying again.
  const errorAction =
    error?.code === 'engineMissing'
      ? { label: t('setup.installNow'), onClick: installEngine }
      : error?.code === 'networkBlocked' && IS_MOBILE
        ? { label: t('error.networkBlocked.action'), onClick: () => void ipc.platformOpenAppSettings() }
        : undefined;

  const isCollapsed = phase !== 'idle';

  return (
    <div
      className={cn(
        'relative mx-auto flex w-full max-w-[620px] flex-1 flex-col px-6 py-10',
        // Idle, the input is the only thing to do, so it sits in the middle of
        // the window. Once a result exists the content grows downward from the
        // top instead of pushing the input around.
        isCollapsed ? 'justify-start' : 'justify-center',
      )}
    >
      <motion.div
        animate={{
          height: isCollapsed ? 0 : 'auto',
          opacity: isCollapsed ? 0 : 1,
          marginBottom: isCollapsed ? 0 : 32,
        }}
        transition={T.spatial}
        className="shrink-0 overflow-hidden"
      >
        <Hero />
      </motion.div>

      <div className={cn('shrink-0', isCollapsed && 'pt-6')}>
        <UrlInput
          ref={inputRef}
          value={url}
          onChange={setUrl}
          onSubmit={startAnalysis}
          onClear={reset}
          onPaste={pasteFromClipboard}
          analyzing={phase === 'analyzing'}
          disabled={!engineReady}
          disabledHint={
            engineReady ? undefined : engineMissing ? t('setup.engineRequired') : t('setup.checking')
          }
        />
      </div>

      <div className="mt-3">
        <PlatformIndicator platform={platform} />
      </div>

      {IS_MOBILE && phase === 'idle' && engineReady && !url.trim() && (
        <p className="mt-4 text-center text-[12.5px] leading-relaxed text-fg-faint">
          {t('input.shareHint')}
        </p>
      )}

      {engineMissing && (
        <motion.div
          variants={RISE}
          initial="initial"
          animate="animate"
          className="mt-4 rounded-[var(--radius-card)] border border-[var(--border)] bg-surface p-4 shadow-soft"
        >
          <h3 className="text-[14px] font-semibold text-fg">{t('setup.title')}</h3>
          <p className="mt-1.5 text-[13px] leading-relaxed text-fg-muted">
            {t(IS_MOBILE ? 'setup.bodyMobile' : 'setup.body')}
          </p>
          {engineInstall ? (
            <InstallProgress progress={engineInstall} className="mt-3" />
          ) : (
            <div className="mt-3 flex gap-2">
              <Button variant="primary" size="sm" onClick={installEngine}>
                {t('setup.installNow')}
              </Button>
              <Button variant="ghost" size="sm" onClick={onOpenSettings}>
                {t('nav.settings')}
              </Button>
            </div>
          )}
        </motion.div>
      )}

      <div className="mt-5 flex flex-col gap-4">
        <AnimatePresence mode="wait">
          {phase === 'analyzing' && <AnalyzingCard key="analyzing" />}

          {phase === 'error' && error && (
            <ErrorCard
              key="error"
              error={error}
              onRetry={() => startAnalysis(url)}
              extraAction={errorAction}
            />
          )}

          {phase === 'ready' && metadata && (
            <motion.div key="ready" className="flex flex-col gap-4">
              <MediaPreviewCard metadata={metadata} />

              <DownloadOptionsPanel
                metadata={metadata}
                options={options}
                onChange={setOptions}
                defaultDownloadDir={settings.downloadDir}
                submitting={submitting}
                onDownload={() => void startDownload(false)}
                onInstallFfmpeg={async () => {
                  const ok = await installTool('ffmpeg');
                  pushToast(
                    ok
                      ? { tone: 'success', title: t('settings.ffmpeg'), body: t('common.done') }
                      : {
                          tone: 'error',
                          title: t('settings.toolInstallFailed'),
                          body: useToolsStore.getState().error ?? t('error.network.message'),
                          durationMs: 9000,
                        },
                  );
                }}
              />

              {metadata.entryCount != null && metadata.entryCount > 1 && (
                <Button
                  variant="secondary"
                  icon={<Images size={15} />}
                  onClick={() => void startDownload(true)}
                  loading={submitting}
                >
                  {t('preview.entries', { n: metadata.entryCount })}
                </Button>
              )}

              <button
                type="button"
                onClick={reset}
                className="pressable mx-auto flex items-center gap-1.5 rounded-md px-2 py-1 text-[12.5px] font-medium text-fg-faint hover:text-fg-muted"
              >
                <RotateCcw size={13} />
                {t('preview.startOver')}
              </button>
            </motion.div>
          )}
        </AnimatePresence>
      </div>

      <AnimatePresence>
        {dragging && (
          <motion.div
            initial={{ opacity: 0 }}
            animate={{ opacity: 1 }}
            exit={{ opacity: 0, transition: T.microOut }}
            transition={T.micro}
            className="fixed inset-0 z-[700] flex items-center justify-center bg-scrim backdrop-blur-sm"
          >
            <motion.div
              initial={{ scale: 0.94, y: 8 }}
              animate={{ scale: 1, y: 0 }}
              transition={T.spatial}
              className="rounded-[var(--radius-panel)] border border-dashed border-[var(--accent)] bg-surface px-8 py-6 text-[13.5px] font-medium text-fg shadow-floating"
            >
              {t('input.dropHint')}
            </motion.div>
          </motion.div>
        )}
      </AnimatePresence>
    </div>
  );
}

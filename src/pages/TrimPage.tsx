import { AnimatePresence, motion } from 'motion/react';
import { open } from '@tauri-apps/plugin-dialog';
import { getCurrentWebview } from '@tauri-apps/api/webview';
import { CircleAlert, FilePlus2, FolderOpen, Scissors } from 'lucide-react';
import { useCallback, useEffect, useRef, useState } from 'react';

import { Timeline } from '@/components/trim/Timeline';
import { Button } from '@/components/ui/Button';
import { IconButton } from '@/components/ui/IconButton';
import { InlineNotice } from '@/components/ui/InlineNotice';
import { ListGroup } from '@/components/ui/ListGroup';
import { PageHeader } from '@/components/ui/PageHeader';
import { Progress } from '@/components/ui/Progress';
import { Segmented } from '@/components/ui/Segmented';
import { SettingRow } from '@/components/ui/SettingRow';
import { Tooltip } from '@/components/ui/Tooltip';
import { useTranslation } from '@/i18n';
import { cn } from '@/lib/cn';
import { formatBytes, formatTimecode, truncateMiddle } from '@/lib/format';
import { COLLAPSE } from '@/lib/motion';
import { openFile, revealFile } from '@/lib/platform';
import * as ipc from '@/services/ipc';
import { MIN_TRIM_SEC, useTrimStore } from '@/stores/useTrimStore';
import { useToolsStore } from '@/stores/useToolsStore';
import type { Settings } from '@/types';

/** What the picker offers. Containers FFmpeg can cut without re-wrapping. */
const VIDEO_EXTENSIONS = ['mp4', 'mkv', 'webm', 'mov', 'avi', 'm4v', 'ts', 'mpg', 'mpeg'];

const DROP_TRANSITION = 'transition-[background-color,box-shadow] duration-150 ease-out-quint';

export function TrimPage({ settings }: { settings: Settings }) {
  const { t } = useTranslation();

  const probe = useTrimStore((state) => state.probe);
  const previewSrc = useTrimStore((state) => state.previewSrc);
  const opening = useTrimStore((state) => state.opening);
  const openError = useTrimStore((state) => state.openError);
  const startSec = useTrimStore((state) => state.startSec);
  const endSec = useTrimStore((state) => state.endSec);
  const precision = useTrimStore((state) => state.precision);
  const outputDir = useTrimStore((state) => state.outputDir);
  const job = useTrimStore((state) => state.job);
  const submitting = useTrimStore((state) => state.submitting);
  const submitError = useTrimStore((state) => state.submitError);

  const openPath = useTrimStore((state) => state.open);
  const closeFile = useTrimStore((state) => state.close);
  const setStart = useTrimStore((state) => state.setStart);
  const setEnd = useTrimStore((state) => state.setEnd);
  const setPrecision = useTrimStore((state) => state.setPrecision);
  const setOutputDir = useTrimStore((state) => state.setOutputDir);
  const startCut = useTrimStore((state) => state.start);
  const cancelCut = useTrimStore((state) => state.cancel);

  const ffmpeg = useToolsStore((state) => state.tools?.ffmpeg ?? null);
  const installing = useToolsStore((state) => state.installing.ffmpeg);
  const installTool = useToolsStore((state) => state.install);
  const ffmpegReady = ffmpeg?.available ?? false;

  const videoRef = useRef<HTMLVideoElement>(null);
  const [positionSec, setPositionSec] = useState(0);
  const [playable, setPlayable] = useState(true);
  const [dragging, setDragging] = useState(false);
  const [pickError, setPickError] = useState<string | null>(null);
  const [installError, setInstallError] = useState<string | null>(null);

  const duration = probe?.durationSec ?? 0;
  const length = Math.max(0, endSec - startSec);
  const running = job.status === 'running';

  // A file dropped on the window arrives as an OS event, and only while this
  // screen is up: dropping one on Home has nothing to do there.
  useEffect(() => {
    const pending = getCurrentWebview().onDragDropEvent((event) => {
      if (event.payload.type === 'over') {
        setDragging(true);
        return;
      }
      setDragging(false);
      // One file: this screen edits one thing at a time, and silently taking
      // the first of five would be a worse answer than taking the first of one.
      if (event.payload.type === 'drop' && event.payload.paths[0]) {
        setPickError(null);
        void openPath(event.payload.paths[0]);
      }
    });
    return () => {
      void pending.then((unlisten) => unlisten());
    };
  }, [openPath]);

  // A new file means a new element: the playhead, and whether the last file
  // could be decoded at all, say nothing about this one.
  useEffect(() => {
    setPositionSec(0);
    setPlayable(true);
  }, [previewSrc]);

  // `timeupdate` fires about four times a second, which is a playhead that
  // hops. While the video is actually playing the position is read per frame
  // instead, and the loop stops the moment it pauses.
  useEffect(() => {
    const video = videoRef.current;
    if (!video) return;
    let frame = 0;

    const follow = () => {
      setPositionSec(video.currentTime);
      frame = requestAnimationFrame(follow);
    };
    const start = () => {
      cancelAnimationFrame(frame);
      frame = requestAnimationFrame(follow);
    };
    const stop = () => {
      cancelAnimationFrame(frame);
      setPositionSec(video.currentTime);
    };

    video.addEventListener('play', start);
    video.addEventListener('pause', stop);
    video.addEventListener('ended', stop);
    video.addEventListener('seeked', stop);
    return () => {
      cancelAnimationFrame(frame);
      video.removeEventListener('play', start);
      video.removeEventListener('pause', stop);
      video.removeEventListener('ended', stop);
      video.removeEventListener('seeked', stop);
    };
  }, [previewSrc]);

  const seek = useCallback((seconds: number) => {
    setPositionSec(seconds);
    const video = videoRef.current;
    // A video that never decoded still has a timeline; only the picture is
    // missing, so a failed seek must not take the marks down with it.
    if (video && Number.isFinite(seconds)) {
      try {
        video.currentTime = seconds;
      } catch {
        // Not seekable yet. The playhead has already moved, which is what the
        // marks are read against.
      }
    }
  }, []);

  const pickFile = useCallback(async () => {
    setPickError(null);
    try {
      const selected = await open({
        multiple: false,
        filters: [{ name: t('trim.videoFiles'), extensions: VIDEO_EXTENSIONS }],
      });
      if (typeof selected === 'string') void openPath(selected);
    } catch (caught) {
      setPickError(ipc.toAppError(caught).message);
    }
  }, [openPath, t]);

  const pickFolder = useCallback(async () => {
    const selected = await open({
      directory: true,
      multiple: false,
      defaultPath: outputDir ?? settings.downloadDir,
    });
    if (typeof selected === 'string') setOutputDir(selected);
  }, [outputDir, setOutputDir, settings.downloadDir]);

  const installFfmpeg = useCallback(async () => {
    setInstallError(null);
    if (await installTool('ffmpeg')) return;
    setInstallError(useToolsStore.getState().error ?? t('error.network.message'));
  }, [installTool, t]);

  return (
    <div className="mx-auto w-full max-w-[760px] px-6 pb-12">
      <PageHeader
        title={t('trim.title')}
        actions={
          probe && (
            <Button size="sm" variant="ghost" onClick={closeFile}>
              {t('trim.close')}
            </Button>
          )
        }
      />

      <AnimatePresence initial={false}>
        {ffmpeg != null && !ffmpegReady && (
          <motion.div variants={COLLAPSE} initial="initial" animate="animate" exit="exit" className="overflow-hidden">
            <div className="mb-5 flex items-center gap-3 rounded-[var(--radius-card)] border border-card-edge bg-surface px-4 py-3">
              <CircleAlert size={16} aria-hidden="true" className="shrink-0 text-warning" />
              <div className="min-w-0 flex-1">
                <p className="text-[13.5px] font-medium text-fg">{t('convert.ffmpegRequired')}</p>
                <p className="mt-0.5 text-[12.5px] leading-relaxed text-fg-muted">
                  {t('trim.ffmpegRequiredBody')}
                </p>
                {installError && (
                  <InlineNotice tone="error" className="mt-1.5">
                    {installError}
                  </InlineNotice>
                )}
              </div>
              <Button size="sm" variant="secondary" loading={installing != null} onClick={() => void installFfmpeg()}>
                {installing != null ? t('settings.toolInstalling') : t('settings.toolInstall')}
              </Button>
            </div>
          </motion.div>
        )}
      </AnimatePresence>

      {!probe ? (
        <section>
          <div
            className={cn(
              'flex flex-col items-center rounded-[var(--radius-card)] border border-card-edge px-6 py-9 text-center',
              DROP_TRANSITION,
              dragging ? 'bg-accent-soft ring-2 ring-accent' : 'bg-surface',
            )}
          >
            <FilePlus2 size={28} strokeWidth={1.5} aria-hidden="true" className="text-fg-faint" />
            <p className="mt-3 text-[14px] font-medium text-fg">{t('trim.dropTitle')}</p>
            <p className="mt-0.5 text-[12.5px] text-fg-muted">{t('trim.dropBody')}</p>
            <Button size="sm" variant="secondary" className="mt-4" loading={opening} onClick={() => void pickFile()}>
              {t('trim.chooseFile')}
            </Button>
          </div>
          {(pickError ?? openError) && (
            <InlineNotice tone="error" className="mt-2 px-4">
              {pickError ?? openError}
            </InlineNotice>
          )}
        </section>
      ) : (
        <>
          {/* -- the picture ------------------------------------------------ */}
          <section>
            <div className="overflow-hidden rounded-[var(--radius-card)] bg-black">
              {playable ? (
                <video
                  ref={videoRef}
                  src={previewSrc ?? undefined}
                  controls
                  preload="metadata"
                  onError={() => setPlayable(false)}
                  className="block max-h-[360px] w-full bg-black"
                />
              ) : (
                // WebView2 cannot decode everything FFmpeg can cut -- H.265 and
                // most MKVs among them. The cut still works exactly the same,
                // so the marks stay; only the picture is gone.
                <div className="flex h-[180px] flex-col items-center justify-center px-6 text-center">
                  <p className="text-[13.5px] font-medium text-white">{t('trim.noPreview')}</p>
                  <p className="mt-1 text-[12.5px] leading-relaxed text-white/60">
                    {t('trim.noPreviewBody')}
                  </p>
                </div>
              )}
            </div>

            <div className="mt-2 flex items-baseline gap-2 px-0.5">
              <Tooltip label={probe.path}>
                <p className="min-w-0 flex-1 truncate text-[13px] font-medium text-fg">
                  {probe.fileName}
                </p>
              </Tooltip>
              <span className="tabular shrink-0 text-[12px] text-fg-faint">
                {formatBytes(probe.sizeBytes)}
              </span>
            </div>
          </section>

          {/* -- the marks -------------------------------------------------- */}
          <section className="mt-5">
            <Timeline
              durationSec={duration}
              startSec={startSec}
              endSec={endSec}
              positionSec={positionSec}
              disabled={running}
              onChangeStart={setStart}
              onChangeEnd={setEnd}
              onSeek={seek}
            />

            {/* Three equal columns, so the two marks and what they add up to
                keep their places while the digits under them change. */}
            <div className="mt-3 grid grid-cols-3 gap-3">
              <Mark
                label={t('trim.startMark')}
                value={startSec}
                actionLabel={t('trim.markHere')}
                disabled={running}
                onSetHere={() => setStart(positionSec)}
              />
              <Mark
                label={t('trim.endMark')}
                value={endSec}
                actionLabel={t('trim.markHere')}
                disabled={running}
                onSetHere={() => setEnd(positionSec)}
              />
              <div className="text-right">
                <p className="text-[11.5px] text-fg-faint">{t('trim.length')}</p>
                <p className="tabular mt-0.5 text-[15px] font-medium text-fg">
                  {formatTimecode(length)}
                </p>
              </div>
            </div>
          </section>

          {/* -- how ---------------------------------------------------------- */}
          <section className="mt-6">
            <ListGroup>
              <SettingRow
                title={t('trim.precision')}
                description={
                  precision === 'fast' ? t('trim.precisionFastHint') : t('trim.precisionExactHint')
                }
                control={
                  <Segmented
                    className="w-[220px]"
                    size="sm"
                    value={precision}
                    onChange={setPrecision}
                    options={[
                      { value: 'fast', label: t('trim.precisionFast') },
                      { value: 'exact', label: t('trim.precisionExact') },
                    ]}
                  />
                }
              />
              <SettingRow
                title={
                  outputDir == null ? (
                    t('convert.besideSource')
                  ) : (
                    <Tooltip label={outputDir}>
                      <span>{truncateMiddle(outputDir, 46)}</span>
                    </Tooltip>
                  )
                }
                control={
                  <div className="flex items-center gap-1.5">
                    {outputDir != null && (
                      <Button size="sm" variant="ghost" onClick={() => setOutputDir(null)}>
                        {t('convert.resetFolder')}
                      </Button>
                    )}
                    <Button size="sm" variant="secondary" onClick={() => void pickFolder()}>
                      {t('options.change')}
                    </Button>
                  </div>
                }
              />
            </ListGroup>

            {running ? (
              <div className="mt-4 flex items-center gap-3">
                <Progress value={job.percent} className="flex-1" label={t('trim.cutting')} />
                <Button size="md" variant="secondary" onClick={() => void cancelCut()}>
                  {t('common.cancel')}
                </Button>
              </div>
            ) : (
              <Button
                className="mt-4"
                variant="cta"
                size="lg"
                fullWidth
                icon={<Scissors size={17} />}
                loading={submitting}
                disabled={!ffmpegReady || length < MIN_TRIM_SEC}
                onClick={() => void startCut()}
              >
                {t('trim.start')}
              </Button>
            )}

            {submitError && (
              <InlineNotice tone="error" className="mt-2 px-4">
                {submitError}
              </InlineNotice>
            )}
            {job.status === 'failed' && job.error && (
              <InlineNotice tone="error" className="mt-2 px-4">
                {job.error.message}
              </InlineNotice>
            )}

            <AnimatePresence initial={false}>
              {job.status === 'completed' && job.outputPath && (
                <motion.div variants={COLLAPSE} initial="initial" animate="animate" exit="exit" className="overflow-hidden">
                  <div className="mt-3 flex items-center gap-3 rounded-[var(--radius-card)] border border-card-edge bg-surface px-4 py-3">
                    <div className="min-w-0 flex-1">
                      <p className="text-[13.5px] font-medium text-fg">{t('trim.done')}</p>
                      <p className="mt-0.5 truncate text-[12.5px] text-fg-muted">
                        {job.outputPath}
                      </p>
                    </div>
                    <Button size="sm" variant="secondary" onClick={() => void openFile(job.outputPath!)}>
                      {t('common.open')}
                    </Button>
                    <IconButton
                      icon={<FolderOpen size={15} />}
                      label={t('downloads.showInFolder')}
                      onClick={() => void revealFile(job.outputPath!)}
                    />
                  </div>
                </motion.div>
              )}
            </AnimatePresence>
          </section>
        </>
      )}
    </div>
  );
}

/** One mark's readout, with the button that sends it to the playhead. */
function Mark({
  label,
  value,
  actionLabel,
  disabled,
  onSetHere,
}: {
  label: string;
  value: number;
  actionLabel: string;
  disabled: boolean;
  onSetHere: () => void;
}) {
  return (
    <div className="min-w-0">
      <p className="text-[11.5px] text-fg-faint">{label}</p>
      <div className="mt-0.5 flex items-center gap-1">
        <span className="tabular text-[15px] font-medium text-fg">{formatTimecode(value)}</span>
        <Button size="sm" variant="ghost" disabled={disabled} onClick={onSetHere}>
          {actionLabel}
        </Button>
      </div>
    </div>
  );
}

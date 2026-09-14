import { AnimatePresence, motion } from 'motion/react';
import { open } from '@tauri-apps/plugin-dialog';
import { getCurrentWebview } from '@tauri-apps/api/webview';
import {
  CircleAlert,
  Eraser,
  FileAudio,
  FilePlus2,
  FileVideo,
  FolderOpen,
  Repeat,
  RotateCcw,
  X,
} from 'lucide-react';
import { useCallback, useEffect, useMemo, useState, type ReactNode } from 'react';

import { ConvertCard } from '@/components/convert/ConvertCard';
import { Badge } from '@/components/ui/Badge';
import { Button } from '@/components/ui/Button';
import { Dropdown, type DropdownOption } from '@/components/ui/Dropdown';
import { Segmented } from '@/components/ui/Segmented';
import { Spinner } from '@/components/ui/Spinner';
import { Toggle } from '@/components/ui/Toggle';
import { Tooltip } from '@/components/ui/Tooltip';
import { useTranslation } from '@/i18n';
import type { TranslationKey } from '@/i18n';
import { cn } from '@/lib/cn';
import {
  AUDIO_BITRATES,
  INPUT_EXTENSIONS,
  RESOLUTION_CAPS,
  acceptsBitrate,
  formatResolution,
  formatsOfKind,
  kindOf,
} from '@/lib/convertOptions';
import { formatBytes, formatDuration, prettyCodec, truncateMiddle } from '@/lib/format';
import * as ipc from '@/services/ipc';
import {
  selectActiveJobs,
  selectFinishedJobs,
  useConvertStore,
  type StagedFile,
} from '@/stores/useConvertStore';
import { useToastStore } from '@/stores/useToastStore';
import { useToolsStore } from '@/stores/useToolsStore';
import type { ConvertFormatInfo, ConvertJob, ConvertQuality, Settings } from '@/types';

const DEFAULT_TARGET = 'mp4';

export function ConvertPage({ settings }: { settings: Settings }) {
  const { t } = useTranslation();

  const files = useConvertStore((state) => state.files);
  const jobs = useConvertStore((state) => state.jobs);
  const submitting = useConvertStore((state) => state.submitting);
  const addFiles = useConvertStore((state) => state.addFiles);
  const removeFile = useConvertStore((state) => state.removeFile);
  const clearFiles = useConvertStore((state) => state.clearFiles);
  const submit = useConvertStore((state) => state.submit);

  const ffmpeg = useToolsStore((state) => state.tools?.ffmpeg ?? null);
  const installing = useToolsStore((state) => state.installing.ffmpeg);
  const installTool = useToolsStore((state) => state.install);
  const pushToast = useToastStore((state) => state.push);

  const [catalogue, setCatalogue] = useState<ConvertFormatInfo[]>([]);
  const [target, setTarget] = useState(DEFAULT_TARGET);
  const [quality, setQuality] = useState<ConvertQuality>('balanced');
  const [maxHeight, setMaxHeight] = useState<number | null>(null);
  const [bitrate, setBitrate] = useState<number | null>(null);
  const [allowStreamCopy, setAllowStreamCopy] = useState(true);
  const [outputDir, setOutputDir] = useState<string | null>(null);
  const [dragging, setDragging] = useState(false);

  // The catalogue is fixed for the life of the process, so it is fetched once.
  useEffect(() => {
    let active = true;
    ipc
      .convertFormats()
      .then((formats) => {
        if (active) setCatalogue(formats);
      })
      .catch(() => {});
    return () => {
      active = false;
    };
  }, []);

  // Files dropped onto the window arrive as an OS event rather than an HTML
  // one, and only while this screen is mounted -- dropping a file on Home has
  // nothing to do there.
  useEffect(() => {
    const pending = getCurrentWebview().onDragDropEvent((event) => {
      if (event.payload.type === 'over') {
        setDragging(true);
      } else if (event.payload.type === 'drop') {
        setDragging(false);
        void addFiles(event.payload.paths);
      } else {
        setDragging(false);
      }
    });
    return () => {
      void pending.then((unlisten) => unlisten());
    };
  }, [addFiles]);

  const targetKind = kindOf(catalogue, target) ?? 'video';
  const active = useMemo(() => selectActiveJobs(jobs), [jobs]);
  const finished = useMemo(() => selectFinishedJobs(jobs), [jobs]);
  const ready = files.filter((file) => file.error === null && !file.probing);
  const ffmpegReady = ffmpeg?.available ?? false;

  const formatOptions = useMemo<DropdownOption<string>[]>(() => {
    const build = (id: string): DropdownOption<string> => ({
      value: id,
      label: id.toUpperCase(),
      description: t(`convert.formatHint.${id}` as TranslationKey),
    });
    return [
      ...formatsOfKind(catalogue, 'video').map(build),
      ...formatsOfKind(catalogue, 'audio').map(build),
    ];
  }, [catalogue, t]);

  const pickFiles = useCallback(async () => {
    const selected = await open({
      multiple: true,
      filters: [{ name: t('convert.mediaFiles'), extensions: [...INPUT_EXTENSIONS] }],
    });
    if (Array.isArray(selected)) void addFiles(selected);
    else if (typeof selected === 'string') void addFiles([selected]);
  }, [addFiles, t]);

  const pickFolder = useCallback(async () => {
    const selected = await open({
      directory: true,
      multiple: false,
      defaultPath: outputDir ?? settings.downloadDir,
    });
    if (typeof selected === 'string') setOutputDir(selected);
  }, [outputDir, settings.downloadDir]);

  const startConversion = useCallback(async () => {
    const result = await submit({
      targetFormat: target,
      outputDir,
      quality,
      maxHeight: targetKind === 'video' ? maxHeight : null,
      audioBitrateKbps: acceptsBitrate(target) ? bitrate : null,
      allowStreamCopy,
    });

    if (result.error) {
      pushToast({ tone: 'error', title: t('convert.failedToQueue'), body: result.error });
    } else if (result.queued > 0) {
      pushToast({
        tone: 'success',
        title: t('convert.queued', { n: result.queued }),
        dedupeKey: 'convert-queued',
      });
    }
  }, [
    allowStreamCopy,
    bitrate,
    maxHeight,
    outputDir,
    pushToast,
    quality,
    submit,
    t,
    target,
    targetKind,
  ]);

  const handlers = {
    onCancel: (id: string) => void ipc.cancelConversion(id),
    onRetry: (id: string) => void ipc.retryConversion(id),
    onRemove: (id: string) => void ipc.removeConversion(id),
  };

  return (
    <div className="mx-auto w-full max-w-[760px] px-6 pb-12 pt-2">
      <AnimatePresence>
        {ffmpeg != null && !ffmpegReady && (
          <motion.div
            initial={{ opacity: 0, y: -6 }}
            animate={{ opacity: 1, y: 0 }}
            exit={{ opacity: 0, height: 0 }}
            className={cn(
              'mb-4 flex items-center gap-3 rounded-[var(--radius-card)] border px-4 py-3',
              'border-[var(--warning)]/35 bg-warning-soft',
            )}
          >
            <CircleAlert size={16} className="shrink-0 text-warning" />
            <div className="min-w-0 flex-1">
              <p className="text-[13px] font-medium text-fg">{t('convert.ffmpegRequired')}</p>
              <p className="text-[12px] leading-relaxed text-fg-muted">
                {t('convert.ffmpegRequiredBody')}
              </p>
            </div>
            <Button
              size="sm"
              variant="secondary"
              loading={installing != null}
              onClick={async () => {
                if (await installTool('ffmpeg')) return;
                pushToast({
                  tone: 'error',
                  title: t('settings.toolInstallFailed'),
                  body: useToolsStore.getState().error ?? t('error.network.message'),
                  durationMs: 9000,
                });
              }}
            >
              {installing != null ? t('settings.toolInstalling') : t('settings.toolInstall')}
            </Button>
          </motion.div>
        )}
      </AnimatePresence>

      {/* -- picking files ------------------------------------------------- */}

      <section
        className={cn(
          'rounded-[var(--radius-panel)] border border-dashed p-5 transition-colors duration-200',
          dragging
            ? 'border-[var(--accent)] bg-accent-soft/40'
            : 'border-[var(--border-strong)] bg-surface-sunken/60',
        )}
      >
        {files.length === 0 ? (
          <div className="flex flex-col items-center gap-2.5 py-6 text-center">
            <FilePlus2 size={22} className="text-fg-faint" />
            <div>
              <p className="text-[13.5px] font-medium text-fg">{t('convert.dropTitle')}</p>
              <p className="mt-0.5 text-[12.5px] text-fg-muted">{t('convert.dropBody')}</p>
            </div>
            <Button size="sm" variant="secondary" className="mt-1" onClick={() => void pickFiles()}>
              {t('convert.chooseFiles')}
            </Button>
          </div>
        ) : (
          <>
            <div className="mb-3 flex items-center gap-2">
              <span className="eyebrow text-fg-faint">
                {t('convert.fileCount', { n: files.length })}
              </span>
              <div className="ml-auto flex items-center gap-1.5">
                <Button size="sm" variant="ghost" onClick={() => void pickFiles()}>
                  {t('convert.addMore')}
                </Button>
                <Button size="sm" variant="ghost" onClick={clearFiles}>
                  {t('convert.clearFiles')}
                </Button>
              </div>
            </div>
            <div className="flex flex-col gap-1.5">
              <AnimatePresence initial={false} mode="popLayout">
                {files.map((file) => (
                  <StagedRow key={file.path} file={file} onRemove={() => removeFile(file.path)} />
                ))}
              </AnimatePresence>
            </div>
          </>
        )}
      </section>

      {/* -- what to convert them into ------------------------------------- */}

      <section className="mt-4 rounded-[var(--radius-panel)] border border-[var(--border)] bg-surface p-4">
        <div className="grid grid-cols-2 gap-3">
          <Dropdown
            label={t('convert.target')}
            value={target}
            options={formatOptions}
            onChange={setTarget}
            menuWidth={300}
          />

          {targetKind === 'video' ? (
            <Dropdown
              label={t('convert.resolution')}
              value={maxHeight == null ? 'source' : String(maxHeight)}
              options={[
                { value: 'source', label: t('convert.resolutionSource') },
                ...RESOLUTION_CAPS.map((height) => ({
                  value: String(height),
                  label: `${height}p`,
                })),
              ]}
              onChange={(value) => setMaxHeight(value === 'source' ? null : Number(value))}
            />
          ) : (
            <Dropdown
              label={t('convert.bitrate')}
              value={bitrate == null ? 'auto' : String(bitrate)}
              disabled={!acceptsBitrate(target)}
              options={[
                {
                  value: 'auto',
                  label: t('convert.bitrateAuto'),
                  description: t('convert.bitrateAutoHint'),
                },
                ...AUDIO_BITRATES.map((kbps) => ({
                  value: String(kbps),
                  label: `${kbps} kbps`,
                })),
              ]}
              onChange={(value) => setBitrate(value === 'auto' ? null : Number(value))}
            />
          )}
        </div>

        {targetKind === 'video' && (
          <div className="mt-3">
            <Segmented
              label={t('convert.quality')}
              value={quality}
              onChange={setQuality}
              options={[
                { value: 'high', label: t('convert.qualityHigh') },
                { value: 'balanced', label: t('convert.qualityBalanced') },
                { value: 'small', label: t('convert.qualitySmall') },
              ]}
            />
            <p className="mt-1.5 text-[11.5px] leading-relaxed text-fg-faint">
              {t('convert.qualityHint')}
            </p>
          </div>
        )}

        <div className="mt-4 flex items-center gap-3 border-t border-[var(--border)] pt-3.5">
          <div className="min-w-0 flex-1">
            <p className="text-[12.5px] font-medium text-fg">{t('convert.repackage')}</p>
            <p className="text-[11.5px] leading-relaxed text-fg-muted">
              {t('convert.repackageHint')}
            </p>
          </div>
          <Toggle
            checked={allowStreamCopy}
            onChange={setAllowStreamCopy}
            label={t('convert.repackage')}
          />
        </div>

        <div className="mt-3 flex items-center gap-2 border-t border-[var(--border)] pt-3.5">
          <FolderOpen size={14} className="shrink-0 text-fg-faint" />
          <span className="min-w-0 flex-1 truncate text-[12px] text-fg-muted">
            {outputDir == null ? (
              t('convert.besideSource')
            ) : (
              <Tooltip label={outputDir}>
                <span className="font-mono text-[11.5px]">{truncateMiddle(outputDir, 46)}</span>
              </Tooltip>
            )}
          </span>
          {outputDir != null && (
            <Button
              size="sm"
              variant="ghost"
              icon={<RotateCcw size={13} />}
              onClick={() => setOutputDir(null)}
            >
              {t('convert.resetFolder')}
            </Button>
          )}
          <Button size="sm" variant="secondary" onClick={() => void pickFolder()}>
            {t('options.change')}
          </Button>
        </div>

        <Button
          className="mt-4"
          variant="cta"
          size="lg"
          fullWidth
          icon={<Repeat size={17} />}
          loading={submitting}
          disabled={ready.length === 0 || !ffmpegReady}
          onClick={() => void startConversion()}
        >
          {ready.length > 1
            ? t('convert.startMany', { n: ready.length })
            : t('convert.start')}
        </Button>
      </section>

      {/* -- what is running ------------------------------------------------ */}

      {jobs.length > 0 && (
        <div className="mt-7 flex flex-col gap-6">
          <JobGroup title={t('convert.inProgress')} jobs={active} handlers={handlers} />
          <JobGroup
            title={t('downloads.finished')}
            jobs={finished}
            handlers={handlers}
            action={
              finished.length > 0 && (
                <Button
                  size="sm"
                  variant="ghost"
                  icon={<Eraser size={14} />}
                  onClick={() => void ipc.clearFinishedConversions()}
                >
                  {t('downloads.clearFinished')}
                </Button>
              )
            }
          />
        </div>
      )}
    </div>
  );
}

/** One file waiting to be converted, with what the probe found in it. */
function StagedRow({ file, onRemove }: { file: StagedFile; onRemove: () => void }) {
  const { t } = useTranslation();
  const probe = file.probe;
  const Icon = probe?.hasVideo ? FileVideo : FileAudio;

  const details: string[] = [];
  if (probe) {
    const resolution = formatResolution(probe.width, probe.height);
    if (resolution) details.push(resolution);
    const codec = prettyCodec(probe.videoCodec ?? probe.audioCodec);
    if (codec) details.push(codec);
    if (probe.durationSec != null) details.push(formatDuration(probe.durationSec));
    details.push(formatBytes(probe.sizeBytes));
  }

  return (
    <motion.div
      layout="position"
      initial={{ opacity: 0, y: 4 }}
      animate={{ opacity: 1, y: 0 }}
      exit={{ opacity: 0, scale: 0.98, transition: { duration: 0.14 } }}
      className={cn(
        'group flex items-center gap-2.5 rounded-[8px] border border-[var(--border)] bg-surface px-3 py-2',
        file.error != null && 'opacity-60',
      )}
    >
      <Icon size={15} className="shrink-0 text-fg-faint" />
      <div className="min-w-0 flex-1">
        <p className="truncate text-[12.5px] font-medium text-fg">{file.name}</p>
        {file.error != null ? (
          <p className="truncate text-[11.5px] text-error">{file.error}</p>
        ) : file.probing ? (
          <span className="flex items-center gap-1.5 text-[11.5px] text-fg-faint">
            <Spinner size={10} />
            {t('convert.reading')}
          </span>
        ) : (
          <p className="metric truncate text-[11.5px] text-fg-faint">{details.join(' · ')}</p>
        )}
      </div>
      {probe != null && !probe.hasVideo && <Badge tone="outline">{t('options.modeAudio')}</Badge>}
      <button
        type="button"
        onClick={onRemove}
        aria-label={t('convert.removeFile')}
        className="shrink-0 rounded-md p-1 text-fg-faint opacity-0 transition-opacity hover:bg-surface-hover hover:text-fg group-hover:opacity-100 focus-visible:opacity-100"
      >
        <X size={13} />
      </button>
    </motion.div>
  );
}

interface JobHandlers {
  onCancel: (id: string) => void;
  onRetry: (id: string) => void;
  onRemove: (id: string) => void;
}

function JobGroup({
  title,
  jobs,
  handlers,
  action,
}: {
  title: string;
  jobs: ConvertJob[];
  handlers: JobHandlers;
  action?: ReactNode;
}) {
  if (jobs.length === 0) return null;

  return (
    <motion.section layout="position">
      <div className="mb-2 flex items-center px-1">
        <h2 className="text-[11px] font-semibold uppercase tracking-[0.08em] text-fg-faint">
          {title}
          <span className="tabular ml-1.5 font-normal text-fg-faint/70">{jobs.length}</span>
        </h2>
        {action && <div className="ml-auto">{action}</div>}
      </div>
      <div className="flex flex-col gap-2">
        <AnimatePresence initial={false} mode="popLayout">
          {jobs.map((job) => (
            <ConvertCard key={job.id} job={job} {...handlers} />
          ))}
        </AnimatePresence>
      </div>
    </motion.section>
  );
}

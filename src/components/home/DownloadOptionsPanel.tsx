import { AnimatePresence, motion } from 'motion/react';
import { open } from '@tauri-apps/plugin-dialog';
import {
  ChevronDown,
  Download,
  FolderOpen,
  Image as ImageIcon,
  Music,
  Sliders,
  TriangleAlert,
  Video,
} from 'lucide-react';
import { useEffect, useMemo, useState } from 'react';

import { useToolInstall } from '@/components/home/useToolInstall';
import { InstallProgress } from '@/components/settings/ToolCard';
import { Button } from '@/components/ui/Button';
import { Dropdown } from '@/components/ui/Dropdown';
import { InlineNotice } from '@/components/ui/InlineNotice';
import { Segmented, type SegmentedOption } from '@/components/ui/Segmented';
import { Tooltip } from '@/components/ui/Tooltip';
import { useTranslation } from '@/i18n';
import { cn } from '@/lib/cn';
import { COLLAPSE } from '@/lib/motion';
import {
  audioStreamOptions,
  availableModes,
  containerOptions,
  decodeQuality,
  encodeQuality,
  qualityOptions,
  videoStreamOptions,
  watermarkState,
} from '@/lib/downloadOptions';
import { formatBytes, truncateMiddle } from '@/lib/format';
import { IS_MOBILE } from '@/lib/platform';
import * as ipc from '@/services/ipc';
import type { DownloadOptions } from '@/stores/useAnalysisStore';
import { useToolsStore } from '@/stores/useToolsStore';
import type { DownloadMode, DownloadRequest, MediaMetadata, WatermarkPreference } from '@/types';

interface DownloadOptionsPanelProps {
  metadata: MediaMetadata;
  options: DownloadOptions;
  onChange: (patch: Partial<DownloadOptions>) => void;
  defaultDownloadDir: string;
  onDownload: () => void;
  submitting: boolean;
  /** Why the last press of Download did not queue anything. */
  downloadError?: string | null;
}

const MODE_ICONS = { video: Video, audio: Music, image: ImageIcon } as const;

export function DownloadOptionsPanel({
  metadata,
  options,
  onChange,
  defaultDownloadDir,
  onDownload,
  submitting,
  downloadError,
}: DownloadOptionsPanelProps) {
  const { t } = useTranslation();
  const [plan, setPlan] = useState<ipc.PlanSummary | null>(null);
  const [planError, setPlanError] = useState<string | null>(null);

  // Read live rather than snapshotting into the plan: installing FFmpeg from
  // the warning below has to clear that warning without the user having to
  // touch another control first.
  const ffmpegAvailable = useToolsStore((state) => state.tools?.ffmpeg.available ?? false);
  const ffmpegInstall = useToolsStore((state) => state.installing.ffmpeg);
  const { install: installFfmpeg, error: ffmpegInstallError } = useToolInstall('ffmpeg');

  const modes = useMemo(() => availableModes(metadata), [metadata]);
  const qualities = useMemo(
    () => qualityOptions(metadata, options.mode),
    [metadata, options.mode],
  );
  const containers = useMemo(
    () => containerOptions(options.mode, ffmpegAvailable),
    [options.mode, ffmpegAvailable],
  );
  const watermark = watermarkState(metadata);

  const request = useMemo<DownloadRequest>(
    () => ({
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
    }),
    [metadata, options],
  );

  // Ask the backend what this selection resolves to. Re-runs on every change,
  // but it is a pure in-process call with no I/O.
  useEffect(() => {
    let active = true;
    ipc
      .summarizePlan(metadata, request)
      .then((summary) => {
        if (!active) return;
        setPlan(summary);
        setPlanError(null);
      })
      .catch((error) => {
        if (!active) return;
        setPlan(null);
        setPlanError(ipc.toAppError(error).message);
      });
    return () => {
      active = false;
    };
    // Re-planning when FFmpeg appears matters: a merge that was impossible a
    // moment ago becomes possible, and the summary has to say so.
  }, [metadata, request, ffmpegAvailable]);

  const modeOptions: SegmentedOption<DownloadMode>[] = modes.map((mode) => {
    const Icon = MODE_ICONS[mode];
    return {
      value: mode,
      label: t(`options.mode${mode[0]!.toUpperCase()}${mode.slice(1)}` as never),
      icon: <Icon size={14} />,
    };
  });

  const pickFolder = async () => {
    const selected = await open({
      directory: true,
      multiple: false,
      defaultPath: options.outputDir ?? defaultDownloadDir,
    });
    if (typeof selected === 'string') onChange({ outputDir: selected });
  };

  const ffmpegBlocked = plan?.needsFfmpeg === true && !ffmpegAvailable;
  const outputDir = options.outputDir ?? defaultDownloadDir;

  return (
    <section className="rounded-[var(--radius-card)] border border-card-edge bg-surface p-4">
      {modeOptions.length > 1 && (
        <Segmented
          value={options.mode}
          options={modeOptions}
          onChange={(mode) => onChange({ mode })}
          label={t('options.mode')}
          className="mb-4"
        />
      )}

      <div className="grid grid-cols-2 gap-3">
        <Dropdown
          label={t('options.quality')}
          value={encodeQuality(options.quality)}
          options={qualities}
          onChange={(key) => onChange({ quality: decodeQuality(key) })}
          disabled={options.advanced}
        />

        <Dropdown
          label={t('options.format')}
          value={options.container ?? ''}
          options={containers}
          onChange={(container) => onChange({ container: container || null })}
        />
      </div>

      {watermark !== 'hidden' && (
        <div className="mt-3">
          {watermark === 'available' ? (
            <Segmented
              label={t('options.watermark')}
              value={options.watermark}
              options={[
                { value: 'any', label: t('options.watermarkAny') },
                { value: 'cleanOnly', label: t('options.watermarkClean') },
              ]}
              onChange={(value) => onChange({ watermark: value as WatermarkPreference })}
              size="sm"
            />
          ) : (
            <InlineNotice>{t('options.watermarkUnavailable')}</InlineNotice>
          )}
        </div>
      )}

      {/* Labelled and filled like the dropdowns above it, so the panel reads as
          one column of fields rather than fields and a box. */}
      <div className="mt-3 flex flex-col gap-1.5">
        <span className="text-[12.5px] font-medium text-fg-muted">{t('options.saveTo')}</span>
        <div
          className={cn(
            'flex items-center gap-2 rounded-[var(--radius-control)] bg-fill pl-3 pr-1',
            IS_MOBILE ? 'h-12' : 'h-9',
          )}
        >
          <FolderOpen size={15} className="shrink-0 text-fg-muted" />
          <Tooltip label={outputDir}>
            <span
              className={cn(
                'min-w-0 flex-1 truncate text-fg',
                IS_MOBILE ? 'text-[15px]' : 'text-[13.5px]',
              )}
            >
              {truncateMiddle(outputDir, 46)}
            </span>
          </Tooltip>
          {/* Android has no folder picker that yields a writable path. */}
          {!IS_MOBILE && (
            <button
              type="button"
              onClick={pickFolder}
              className="pressable h-7 shrink-0 rounded-[7px] px-2 text-[12.5px] font-medium text-accent hover:bg-accent-soft"
            >
              {t('options.change')}
            </button>
          )}
        </div>
      </div>

      <button
        type="button"
        onClick={() => onChange({ advanced: !options.advanced })}
        aria-expanded={options.advanced}
        className="pressable -ml-1.5 mt-3 flex items-center gap-1.5 rounded-md px-1.5 py-1 text-[12.5px] font-medium text-fg-muted hover:text-fg"
      >
        <Sliders size={13} />
        {options.advanced ? t('options.advancedHide') : t('options.advanced')}
        <ChevronDown
          size={13}
          className={cn(
            'transition-transform duration-150 ease-out-quint',
            options.advanced && 'rotate-180',
          )}
        />
      </button>

      <AnimatePresence initial={false}>
        {options.advanced && (
          <motion.div
            variants={COLLAPSE}
            initial="initial"
            animate="animate"
            exit="exit"
            className="overflow-hidden"
          >
            <div className="mt-2 grid gap-3">
              <Dropdown
                label={t('options.videoStream')}
                value={options.videoFormatId ?? plan?.videoFormatId ?? ''}
                options={videoStreamOptions(metadata)}
                onChange={(id) => onChange({ videoFormatId: id || null })}
                disabled={options.mode === 'audio'}
              />
              <Dropdown
                label={t('options.audioStream')}
                value={options.audioFormatId ?? plan?.audioFormatId ?? ''}
                options={audioStreamOptions(metadata)}
                onChange={(id) => onChange({ audioFormatId: id || null })}
              />
            </div>
          </motion.div>
        )}
      </AnimatePresence>

      {ffmpegBlocked && (
        <div className="mt-3.5 flex items-start gap-2.5 rounded-[var(--radius-control)] bg-warning-soft p-3">
          <TriangleAlert size={15} className="mt-px shrink-0 text-warning" />
          <div className="min-w-0 flex-1">
            <p className="text-[12.5px] leading-relaxed text-warning">
              {t('error.ffmpegMissing.message')}
            </p>
            {ffmpegInstall ? (
              <InstallProgress progress={ffmpegInstall} className="mt-2" />
            ) : (
              <Button size="sm" className="mt-2" onClick={() => void installFfmpeg()}>
                {t('setup.installNow')}
              </Button>
            )}
            {ffmpegInstallError && !ffmpegInstall && (
              <InlineNotice tone="error" className="mt-2">
                {ffmpegInstallError}
              </InlineNotice>
            )}
          </div>
        </div>
      )}

      {planError && (
        <InlineNotice tone="error" className="mt-3.5">
          {planError}
        </InlineNotice>
      )}

      <Button
        variant="cta"
        size="lg"
        fullWidth
        icon={<Download size={17} />}
        loading={submitting}
        disabled={ffmpegBlocked || plan == null}
        onClick={onDownload}
        className="mt-4"
      >
        {submitting ? t('action.preparing') : t('action.download')}
      </Button>

      {downloadError && (
        <InlineNotice tone="error" className="mt-2.5">
          {downloadError}
        </InlineNotice>
      )}

      {/* What the button will fetch, as one plain line: the resolved format,
          and roughly how big it is when the source said. */}
      {plan && (
        <p className="mt-2.5 flex flex-wrap items-center justify-center gap-x-3 text-[12.5px] text-fg-muted">
          <span>{plan.label}</span>
          {plan.estimatedBytes != null && (
            <span className="tabular">~{formatBytes(plan.estimatedBytes)}</span>
          )}
        </p>
      )}
    </section>
  );
}

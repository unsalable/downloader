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

import { InstallProgress } from '@/components/settings/ToolCard';
import { Badge } from '@/components/ui/Badge';
import { Button } from '@/components/ui/Button';
import { Dropdown } from '@/components/ui/Dropdown';
import { Segmented, type SegmentedOption } from '@/components/ui/Segmented';
import { Tooltip } from '@/components/ui/Tooltip';
import { useTranslation } from '@/i18n';
import { cn } from '@/lib/cn';
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
  onInstallFfmpeg: () => void;
}

const MODE_ICONS = { video: Video, audio: Music, image: ImageIcon } as const;

export function DownloadOptionsPanel({
  metadata,
  options,
  onChange,
  defaultDownloadDir,
  onDownload,
  submitting,
  onInstallFfmpeg,
}: DownloadOptionsPanelProps) {
  const { t } = useTranslation();
  const [plan, setPlan] = useState<ipc.PlanSummary | null>(null);
  const [planError, setPlanError] = useState<string | null>(null);

  // Read live rather than snapshotting into the plan: installing FFmpeg from
  // the warning below has to clear that warning without the user having to
  // touch another control first.
  const ffmpegAvailable = useToolsStore((state) => state.tools?.ffmpeg.available ?? false);
  const ffmpegInstall = useToolsStore((state) => state.installing.ffmpeg);

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
    <motion.section
      initial={{ opacity: 0, y: 10 }}
      animate={{ opacity: 1, y: 0 }}
      transition={{ duration: 0.35, delay: 0.06, ease: [0.22, 1, 0.36, 1] }}
      className="rounded-[var(--radius-panel)] border border-[var(--border)] bg-surface p-4 shadow-raised edge-light"
    >
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
            <div className="flex items-start gap-2 rounded-lg bg-warning-soft px-3 py-2 text-[12.5px] text-warning">
              <TriangleAlert size={14} className="mt-px shrink-0" />
              <span>{t('options.watermarkUnavailable')}</span>
            </div>
          )}
        </div>
      )}

      <div className="mt-3">
        <span className="text-[11px] font-medium uppercase tracking-[0.07em] text-fg-faint">
          {t('options.saveTo')}
        </span>
        <div className="mt-1.5 flex h-10 items-center gap-2 rounded-[10px] border border-[var(--border)] bg-surface px-3">
          <FolderOpen size={15} className="shrink-0 text-fg-faint" />
          <Tooltip label={outputDir}>
            <span className="min-w-0 flex-1 truncate text-[13px] text-fg-muted">
              {truncateMiddle(outputDir, 46)}
            </span>
          </Tooltip>
          {/* Android has no folder picker that yields a writable path. */}
          {!IS_MOBILE && (
            <button
              type="button"
              onClick={pickFolder}
              className="shrink-0 rounded-md px-2 py-1 text-[12.5px] font-medium text-accent transition-colors hover:bg-accent-soft"
            >
              {t('options.change')}
            </button>
          )}
        </div>
      </div>

      <button
        type="button"
        onClick={() => onChange({ advanced: !options.advanced })}
        className="mt-3.5 flex items-center gap-1.5 text-[12.5px] font-medium text-fg-faint transition-colors hover:text-fg-muted"
      >
        <Sliders size={13} />
        {options.advanced ? t('options.advancedHide') : t('options.advanced')}
        <ChevronDown
          size={13}
          className={cn('transition-transform duration-200', options.advanced && 'rotate-180')}
        />
      </button>

      <AnimatePresence initial={false}>
        {options.advanced && (
          <motion.div
            initial={{ opacity: 0, height: 0 }}
            animate={{ opacity: 1, height: 'auto' }}
            exit={{ opacity: 0, height: 0 }}
            transition={{ duration: 0.24, ease: [0.22, 1, 0.36, 1] }}
            className="overflow-hidden"
          >
            <div className="mt-3 grid gap-3 rounded-[var(--radius-card)] border border-[var(--border)] bg-surface-sunken p-3">
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
        <div className="mt-3.5 flex items-start gap-2.5 rounded-lg bg-warning-soft p-3">
          <TriangleAlert size={15} className="mt-px shrink-0 text-warning" />
          <div className="min-w-0 flex-1">
            <p className="text-[12.5px] leading-relaxed text-warning">
              {t('error.ffmpegMissing.message')}
            </p>
            {ffmpegInstall ? (
              <InstallProgress progress={ffmpegInstall} className="mt-2" />
            ) : (
              <button
                type="button"
                onClick={onInstallFfmpeg}
                className="mt-1.5 text-[12.5px] font-semibold text-warning underline underline-offset-2"
              >
                {t('setup.installNow')}
              </button>
            )}
          </div>
        </div>
      )}

      {planError && (
        <p className="mt-3.5 rounded-lg bg-error-soft px-3 py-2 text-[12.5px] text-error">
          {planError}
        </p>
      )}

      <div className="mt-4 flex items-center gap-3">
        <Button
          variant="cta"
          size="lg"
          fullWidth
          icon={<Download size={17} />}
          loading={submitting}
          disabled={ffmpegBlocked || plan == null}
          onClick={onDownload}
        >
          {submitting ? t('action.preparing') : t('action.download')}
        </Button>
      </div>

      <div className="mt-3 flex flex-wrap items-center justify-center gap-2 text-[12px] text-fg-faint">
        {plan && <Badge tone="outline">{plan.label}</Badge>}
        {plan?.needsMerge && <Badge tone="outline">{t('stage.merging')}</Badge>}
        <span className="tabular">
          {plan?.estimatedBytes != null
            ? `${t('options.estimatedSize')} ${formatBytes(plan.estimatedBytes)}`
            : t('options.unknownSize')}
        </span>
      </div>
    </motion.section>
  );
}

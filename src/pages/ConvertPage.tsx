import { AnimatePresence, motion } from 'motion/react';
import { open } from '@tauri-apps/plugin-dialog';
import { getCurrentWebview } from '@tauri-apps/api/webview';
import { CircleAlert, FileAudio, FilePlus2, FileVideo, Repeat, X } from 'lucide-react';
import { useCallback, useEffect, useMemo, useRef, useState } from 'react';

import { ConvertCard } from '@/components/convert/ConvertCard';
import { Button } from '@/components/ui/Button';
import { Dropdown, type DropdownOption } from '@/components/ui/Dropdown';
import { IconButton } from '@/components/ui/IconButton';
import { InlineNotice } from '@/components/ui/InlineNotice';
import { ListGroup, ListGroupLabel, ROW_LINE } from '@/components/ui/ListGroup';
import { PageHeader } from '@/components/ui/PageHeader';
import { Segmented } from '@/components/ui/Segmented';
import { SettingRow, ToggleRow } from '@/components/ui/SettingRow';
import { Spinner } from '@/components/ui/Spinner';
import { Tooltip } from '@/components/ui/Tooltip';
import { useTranslation } from '@/i18n';
import type { TranslationKey } from '@/i18n';
import { cn } from '@/lib/cn';
import { COLLAPSE, LIST_ITEM } from '@/lib/motion';
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
import { IS_MOBILE } from '@/lib/platform';
import * as ipc from '@/services/ipc';
import { selectFinishedJobs, useConvertStore, type StagedFile } from '@/stores/useConvertStore';
import { useToolsStore } from '@/stores/useToolsStore';
import type { ConvertFormatInfo, ConvertQuality, Settings } from '@/types';

const DEFAULT_TARGET = 'mp4';

/** Where a row's text starts: its padding, the 44px tile, the gap. */
const TEXT_INSET = 72;

/** A phone stacks the control under its label, where it takes the full width. */
const CONTROL_WIDTH = IS_MOBILE ? 'w-full' : 'w-[220px]';

const DROP_TRANSITION = 'transition-[background-color,box-shadow] duration-150 ease-out-quint';

const STAGED_LINE = cn(ROW_LINE, 'mt-0.5');

/** Stable across renders so the memoised rows only re-render when their job does. */
const JOB_HANDLERS = {
  onCancel: (id: string) => void ipc.cancelConversion(id),
  onRetry: (id: string) => void ipc.retryConversion(id),
  onRemove: (id: string) => void ipc.removeConversion(id),
};

export function ConvertPage({ settings }: { settings: Settings }) {
  const { t } = useTranslation();

  const files = useConvertStore((state) => state.files);
  const submitting = useConvertStore((state) => state.submitting);
  const addFiles = useConvertStore((state) => state.addFiles);
  const removeFile = useConvertStore((state) => state.removeFile);
  const clearFiles = useConvertStore((state) => state.clearFiles);
  const submit = useConvertStore((state) => state.submit);

  const ffmpeg = useToolsStore((state) => state.tools?.ffmpeg ?? null);
  const installing = useToolsStore((state) => state.installing.ffmpeg);
  const installTool = useToolsStore((state) => state.install);

  const [catalogue, setCatalogue] = useState<ConvertFormatInfo[]>([]);
  const [target, setTarget] = useState(DEFAULT_TARGET);
  const [quality, setQuality] = useState<ConvertQuality>('balanced');
  const [maxHeight, setMaxHeight] = useState<number | null>(null);
  const [bitrate, setBitrate] = useState<number | null>(null);
  const [allowStreamCopy, setAllowStreamCopy] = useState(true);
  const [outputDir, setOutputDir] = useState<string | null>(null);
  const [dragging, setDragging] = useState(false);

  // Each failure is reported beside the control that caused it, and cleared
  // the next time that control is used.
  const [pickError, setPickError] = useState<string | null>(null);
  const [submitError, setSubmitError] = useState<string | null>(null);
  const [installError, setInstallError] = useState<string | null>(null);

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
    if (IS_MOBILE) return;
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

  // Both notices under the file section talk about the staged list, so they go
  // when that list changes -- rather than sitting under a control the user can
  // no longer press.
  const forgetFileNotices = useCallback(() => {
    setPickError(null);
    setSubmitError(null);
  }, []);

  const pickFiles = useCallback(async () => {
    setPickError(null);
    try {
      // The Android picker returns content URIs that FFmpeg cannot open; the
      // platform side copies the choices somewhere it can and returns those.
      if (IS_MOBILE) {
        const picked = await ipc.platformPickMediaFiles();
        if (picked.length > 0) void addFiles(picked);
        return;
      }
      const selected = await open({
        multiple: true,
        filters: [{ name: t('convert.mediaFiles'), extensions: [...INPUT_EXTENSIONS] }],
      });
      if (Array.isArray(selected)) void addFiles(selected);
      else if (typeof selected === 'string') void addFiles([selected]);
    } catch (caught) {
      setPickError(ipc.toAppError(caught).message);
    }
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
    setSubmitError(null);
    const result = await submit({
      targetFormat: target,
      outputDir,
      quality,
      maxHeight: targetKind === 'video' ? maxHeight : null,
      audioBitrateKbps: acceptsBitrate(target) ? bitrate : null,
      allowStreamCopy,
    });
    // Success needs no announcement: the files leave this list and turn up as
    // rows in the one below.
    if (result.error) setSubmitError(result.error);
  }, [allowStreamCopy, bitrate, maxHeight, outputDir, quality, submit, target, targetKind]);

  const installFfmpeg = useCallback(async () => {
    setInstallError(null);
    if (await installTool('ffmpeg')) return;
    setInstallError(useToolsStore.getState().error ?? t('error.network.message'));
  }, [installTool, t]);

  return (
    <div className={cn('mx-auto w-full max-w-[760px] pb-12', IS_MOBILE ? 'px-4 pt-2' : 'px-6')}>
      <PageHeader title={t('convert.title')} />

      <AnimatePresence initial={false}>
        {ffmpeg != null && !ffmpegReady && (
          <motion.div
            variants={COLLAPSE}
            initial="initial"
            animate="animate"
            exit="exit"
            className="overflow-hidden"
          >
            <div className="mb-5 flex items-center gap-3 rounded-[var(--radius-card)] border border-card-edge bg-surface px-4 py-3">
              <CircleAlert size={16} aria-hidden="true" className="shrink-0 text-warning" />
              <div className="min-w-0 flex-1">
                <p className="text-[13.5px] font-medium text-fg">{t('convert.ffmpegRequired')}</p>
                <p className="mt-0.5 text-[12.5px] leading-relaxed text-fg-muted">
                  {t('convert.ffmpegRequiredBody')}
                </p>
                {installError && (
                  <InlineNotice tone="error" className="mt-1.5">
                    {installError}
                  </InlineNotice>
                )}
              </div>
              <Button
                size="sm"
                variant="secondary"
                loading={installing != null}
                onClick={() => void installFfmpeg()}
              >
                {installing != null ? t('settings.toolInstalling') : t('settings.toolInstall')}
              </Button>
            </div>
          </motion.div>
        )}
      </AnimatePresence>

      {/* -- picking files ------------------------------------------------- */}

      <section>
        {files.length === 0 ? (
          <div
            className={cn(
              'flex flex-col items-center rounded-[var(--radius-card)] border border-card-edge px-6 py-9 text-center',
              DROP_TRANSITION,
              dragging ? 'bg-accent-soft ring-2 ring-accent' : 'bg-surface',
            )}
          >
            <FilePlus2 size={28} strokeWidth={1.5} aria-hidden="true" className="text-fg-faint" />
            <p className="mt-3 text-[14px] font-medium text-fg">
              {t(IS_MOBILE ? 'convert.pickTitle' : 'convert.dropTitle')}
            </p>
            <p className="mt-0.5 text-[12.5px] text-fg-muted">
              {t(IS_MOBILE ? 'convert.pickBody' : 'convert.dropBody')}
            </p>
            <Button size="sm" variant="secondary" className="mt-4" onClick={() => void pickFiles()}>
              {t('convert.chooseFiles')}
            </Button>
          </div>
        ) : (
          <>
            <div className="mb-1.5 flex items-center gap-1.5">
              <ListGroupLabel className="min-w-0 flex-1 truncate">
                {t('convert.fileCount', { n: files.length })}
              </ListGroupLabel>
              <Button size="sm" variant="ghost" onClick={() => void pickFiles()}>
                {t('convert.addMore')}
              </Button>
              <Button
                size="sm"
                variant="ghost"
                onClick={() => {
                  clearFiles();
                  forgetFileNotices();
                }}
              >
                {t('convert.clearFiles')}
              </Button>
            </div>
            <ListGroup
              inset={TEXT_INSET}
              className={cn('relative', DROP_TRANSITION, dragging && 'ring-2 ring-accent')}
            >
              <AnimatePresence initial={false} mode="popLayout">
                {files.map((file) => (
                  <StagedRow
                    key={file.path}
                    file={file}
                    onRemove={() => {
                      removeFile(file.path);
                      forgetFileNotices();
                    }}
                  />
                ))}
              </AnimatePresence>
            </ListGroup>
          </>
        )}
        {pickError && (
          <InlineNotice tone="error" className="mt-2 px-4">
            {pickError}
          </InlineNotice>
        )}
      </section>

      {/* -- what to convert them into ------------------------------------- */}

      <section className="mt-6">
        <ListGroup>
          <SettingRow
            title={t('convert.target')}
            stacked={IS_MOBILE}
            control={
              <Dropdown
                className={CONTROL_WIDTH}
                value={target}
                options={formatOptions}
                onChange={setTarget}
                menuWidth={300}
                align="end"
              />
            }
          />

          {targetKind === 'video' ? (
            <SettingRow
              title={t('convert.resolution')}
              stacked={IS_MOBILE}
              control={
                <Dropdown
                  className={CONTROL_WIDTH}
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
              }
            />
          ) : (
            <SettingRow
              title={t('convert.bitrate')}
              stacked={IS_MOBILE}
              control={
                <Dropdown
                  className={CONTROL_WIDTH}
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
              }
            />
          )}

          {targetKind === 'video' && (
            <SettingRow
              title={t('convert.quality')}
              description={t('convert.qualityHint')}
              stacked={IS_MOBILE}
              control={
                <Segmented
                  className={CONTROL_WIDTH}
                  size="sm"
                  value={quality}
                  onChange={setQuality}
                  options={[
                    { value: 'high', label: t('convert.qualityHigh') },
                    { value: 'balanced', label: t('convert.qualityBalanced') },
                    { value: 'small', label: t('convert.qualitySmall') },
                  ]}
                />
              }
            />
          )}

          <ToggleRow
            title={t('convert.repackage')}
            description={t('convert.repackageHint')}
            checked={allowStreamCopy}
            onChange={setAllowStreamCopy}
          />

          {!IS_MOBILE && (
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
          )}
        </ListGroup>

        {IS_MOBILE && (
          // Picked files are private copies, so results always go to the
          // Downloads folder; there is no other writable place to offer.
          <p className="mt-2 px-4 text-[12.5px] leading-relaxed text-fg-muted [overflow-wrap:anywhere]">
            {t('convert.savedTo', { folder: settings.downloadDir })}
          </p>
        )}

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
          {ready.length > 1 ? t('convert.startMany', { n: ready.length }) : t('convert.start')}
        </Button>
        {submitError && (
          <InlineNotice tone="error" className="mt-2 px-4">
            {submitError}
          </InlineNotice>
        )}
      </section>

      {/* -- what is running ------------------------------------------------ */}

      <JobList smoothScroll={!settings.reduceMotion} />
    </div>
  );
}

/** One file waiting to be converted, with what the probe found in it. */
function StagedRow({ file, onRemove }: { file: StagedFile; onRemove: () => void }) {
  const { t } = useTranslation();
  const probe = file.probe;
  const Icon = probe != null && !probe.hasVideo ? FileAudio : FileVideo;

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
      layout={IS_MOBILE ? false : 'position'}
      variants={LIST_ITEM}
      initial="initial"
      animate="animate"
      exit="exit"
      className={cn('group flex items-center gap-3 py-2.5 pl-4', IS_MOBILE ? 'pr-1.5' : 'pr-3')}
    >
      <div
        className={cn(
          'flex size-11 shrink-0 items-center justify-center',
          'rounded-[var(--radius-thumb)] bg-surface-sunken text-fg-muted',
          file.error != null && 'opacity-60',
        )}
      >
        <Icon size={19} aria-hidden="true" />
      </div>
      <div className="min-w-0 flex-1">
        <p className="truncate text-[13.5px] font-medium leading-[18px] text-fg">{file.name}</p>
        {file.error != null ? (
          <p className="mt-0.5 truncate text-[12.5px] leading-[18px] text-error">{file.error}</p>
        ) : file.probing ? (
          <p className={cn(STAGED_LINE, 'items-center gap-x-1.5')}>
            <Spinner size={11} />
            {t('convert.reading')}
          </p>
        ) : (
          <p className={cn(STAGED_LINE, 'gap-x-3')}>
            {details.map((detail, index) => (
              <span key={index}>{detail}</span>
            ))}
          </p>
        )}
      </div>
      <IconButton
        icon={<X size={15} />}
        label={t('convert.removeFile')}
        size={IS_MOBILE ? 'sm' : 'md'}
        className="reveal-on-hover group-focus-within:opacity-100"
        onClick={onRemove}
      />
    </motion.div>
  );
}

/**
 * Every conversion in one list, newest first. It subscribes to the jobs
 * itself, so a progress tick renders this list and not the form above it.
 */
function JobList({ smoothScroll }: { smoothScroll: boolean }) {
  const { t } = useTranslation();
  const jobs = useConvertStore((state) => state.jobs);
  const loaded = useConvertStore((state) => state.loaded);

  const newestFirst = useMemo(() => [...jobs].sort((a, b) => b.createdAt - a.createdAt), [jobs]);
  const hasFinished = useMemo(() => selectFinishedJobs(jobs).length > 0, [jobs]);

  // The Convert button sits at the foot of a tall form, and the rows it
  // produces land below it -- on a short window, out of sight. Bringing them
  // into view is the confirmation that the press did something.
  const sectionRef = useRef<HTMLElement>(null);
  const previousCount = useRef<number | null>(null);
  const count = jobs.length;
  useEffect(() => {
    if (!loaded) return;
    const grew = previousCount.current != null && count > previousCount.current;
    previousCount.current = count;
    if (!grew) return;
    sectionRef.current?.scrollIntoView({
      block: 'nearest',
      behavior: smoothScroll ? 'smooth' : 'auto',
    });
  }, [count, loaded, smoothScroll]);

  if (count === 0) return null;

  return (
    <section ref={sectionRef} className="mt-8 scroll-mb-6">
      <div className="mb-1.5 flex min-h-8 items-center gap-1.5">
        <ListGroupLabel className="min-w-0 flex-1 truncate">{t('convert.jobs')}</ListGroupLabel>
        {hasFinished && (
          <Button
            size="sm"
            variant="ghost"
            aria-label={t('downloads.clearFinished')}
            onClick={() => void ipc.clearFinishedConversions()}
          >
            {t('downloads.clear')}
          </Button>
        )}
      </div>
      {/* `relative` gives a row that is leaving something to be positioned
          against while the rows under it close the gap. */}
      <ListGroup inset={TEXT_INSET} className="relative">
        <AnimatePresence initial={false} mode="popLayout">
          {newestFirst.map((job) => (
            <ConvertCard key={job.id} job={job} {...JOB_HANDLERS} />
          ))}
        </AnimatePresence>
      </ListGroup>
    </section>
  );
}

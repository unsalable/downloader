import { AnimatePresence, motion } from 'motion/react';
import {
  ArrowRight,
  Check,
  FileAudio,
  FileVideo,
  FolderOpen,
  RotateCw,
  SquareArrowOutUpRight,
  Trash2,
  X,
} from 'lucide-react';
import { memo, useState } from 'react';

import { Badge } from '@/components/ui/Badge';
import { IconButton } from '@/components/ui/IconButton';
import { Progress } from '@/components/ui/Progress';
import { Spinner } from '@/components/ui/Spinner';
import { useTranslation } from '@/i18n';
import type { TranslationKey } from '@/i18n';
import { cn } from '@/lib/cn';
import { basename, clampPercent, formatBytes } from '@/lib/format';
import { COLLAPSE, LIST_ITEM, SPRING } from '@/lib/motion';
import { IS_MOBILE, openFile, revealFile } from '@/lib/platform';
import type { ConvertJob } from '@/types';

interface ConvertCardProps {
  job: ConvertJob;
  onCancel: (id: string) => void;
  onRetry: (id: string) => void;
  onRemove: (id: string) => void;
}

const STATUS_TONE = {
  completed: 'success',
  failed: 'error',
  canceled: 'neutral',
} as const;

/** The rule down the left edge, which is how a row's state reads at a glance. */
const EDGE = {
  completed: 'bg-[var(--success)]',
  failed: 'bg-[var(--error)]',
  canceled: 'bg-[var(--border-strong)]',
  running: 'bg-[var(--accent)]',
  queued: 'bg-transparent',
} as const;

/**
 * One row in the conversion list.
 *
 * Memoised on the job: progress replaces only the running job, so an encode in
 * flight re-renders its own card and leaves the rest alone.
 */
export const ConvertCard = memo(function ConvertCard({
  job,
  onCancel,
  onRetry,
  onRemove,
}: ConvertCardProps) {
  const { t } = useTranslation();
  const [showError, setShowError] = useState(false);

  const isRunning = job.status === 'running';
  const isDone = job.status === 'completed';
  const isTerminal = isDone || job.status === 'failed' || job.status === 'canceled';
  const percent = clampPercent(job.percent);
  const Icon = job.kind === 'audio' ? FileAudio : FileVideo;

  const errorTitle = job.error
    ? t(`error.${job.error.code}.title` as TranslationKey) === `error.${job.error.code}.title`
      ? job.error.title
      : t(`error.${job.error.code}.title` as TranslationKey)
    : '';

  // Size only means something once both halves are known; showing "-- -> 4 MB"
  // would read as though the source had no size.
  const sizeDelta =
    isDone && job.outputSizeBytes != null && job.inputSizeBytes > 0
      ? Math.round((job.outputSizeBytes / job.inputSizeBytes) * 100)
      : null;

  return (
    <motion.article
      layout={IS_MOBILE ? false : 'position'}
      variants={LIST_ITEM}
      initial="initial"
      animate="animate"
      exit="exit"
      className={cn(
        'group relative overflow-hidden rounded-[var(--radius-card)] border border-[var(--border)]',
        'bg-surface transition-colors duration-150 ease-out-quint hover:border-[var(--border-strong)]',
      )}
    >
      <span
        aria-hidden="true"
        className={cn(
          'absolute inset-y-0 left-0 w-[3px] transition-colors duration-250 ease-out-quint',
          EDGE[job.status],
        )}
      />

      <div className="flex gap-3 p-3 pl-4">
        <div
          className={cn(
            'relative flex size-11 shrink-0 items-center justify-center rounded-[7px]',
            'bg-surface-sunken text-fg-faint',
            isTerminal && !isDone && 'opacity-45',
          )}
        >
          {isDone ? (
            <motion.span
              initial={{ scale: 0.55, opacity: 0 }}
              animate={{ scale: 1, opacity: 1 }}
              transition={SPRING.snap}
            >
              <Check size={18} strokeWidth={2.5} className="text-success" />
            </motion.span>
          ) : (
            <Icon size={18} />
          )}
        </div>

        <div className="flex min-w-0 flex-1 flex-col gap-1.5">
          <div className="flex items-start gap-2">
            <h3 className="line-clamp-1 min-w-0 flex-1 text-[13.5px] font-medium leading-snug text-fg">
              {job.inputName}
            </h3>
            {isTerminal && (
              <IconButton
                icon={<Trash2 size={13} />}
                label={t('convert.remove')}
                size="sm"
                tone="danger"
                className="reveal-on-hover transition-opacity duration-150 ease-out-quint"
                onClick={() => onRemove(job.id)}
              />
            )}
          </div>

          {/* One meta line: what is becoming what, and the state of the job. */}
          <div className="flex flex-wrap items-center gap-x-2 gap-y-1">
            <span className="metric flex items-center gap-1 text-[12px] text-fg-faint">
              <span className="uppercase">{extensionOf(job.inputName)}</span>
              <ArrowRight size={11} />
              <span className="uppercase text-fg-muted">{job.options.targetFormat}</span>
            </span>
            <span className="metric text-[12px] text-fg-faint">
              {formatBytes(job.inputSizeBytes)}
              {job.outputSizeBytes != null && ` -> ${formatBytes(job.outputSizeBytes)}`}
            </span>
            {job.status in STATUS_TONE && (
              <Badge tone={STATUS_TONE[job.status as keyof typeof STATUS_TONE]}>
                {t(`convert.status.${job.status}` as TranslationKey)}
              </Badge>
            )}
            {job.status === 'queued' && <Badge tone="neutral">{t('downloads.waiting')}</Badge>}
            {isDone && job.streamCopied && (
              <Badge tone="accent">{t('convert.repackaged')}</Badge>
            )}
            {sizeDelta != null && (
              <span className="metric text-[11.5px] text-fg-faint">
                {t('convert.ofOriginal', { percent: sizeDelta })}
              </span>
            )}
            {isRunning && (
              <span className="flex items-center gap-1.5 text-[11.5px] text-fg-muted">
                <Spinner size={11} />
                {t('convert.working')}
              </span>
            )}
          </div>

          <div className="mt-auto pt-1">
            {isRunning && (
              <>
                <div className="metric mb-1.5 text-[15px] font-semibold text-fg">
                  {job.percent == null ? '--' : `${percent.toFixed(0)}%`}
                </div>
                <Progress
                  value={job.percent == null ? null : percent}
                  tone="accent"
                  label={job.inputName}
                />
              </>
            )}

            {isDone && job.outputPath && (
              <div className="flex items-center gap-2">
                {/* Beside two buttons a phone has room for a few letters of the
                    name, which the title above already gives in full. */}
                {!IS_MOBILE && (
                  <span className="metric min-w-0 flex-1 truncate text-[11.5px] text-fg-faint">
                    {basename(job.outputPath)}
                  </span>
                )}
                <button
                  type="button"
                  onClick={() => void openFile(job.outputPath!)}
                  className="pressable flex shrink-0 items-center gap-1 rounded-[5px] px-1.5 py-1 text-[12px] font-medium text-accent hover:bg-accent-soft"
                >
                  <SquareArrowOutUpRight size={12} />
                  {t('downloads.openFile')}
                </button>
                <button
                  type="button"
                  onClick={() => void revealFile(job.outputPath!)}
                  className="pressable flex shrink-0 items-center gap-1 rounded-[5px] px-1.5 py-1 text-[12px] font-medium text-fg-muted hover:bg-surface-hover hover:text-fg"
                >
                  <FolderOpen size={12} />
                  {t('downloads.openFolder')}
                </button>
              </div>
            )}

            {job.status === 'failed' && job.error && (
              <div className="flex items-baseline gap-2">
                <p className="min-w-0 flex-1 truncate text-[12.5px] leading-snug text-fg">
                  {errorTitle}
                </p>
                {job.error.technical && (
                  <button
                    type="button"
                    onClick={() => setShowError((value) => !value)}
                    aria-expanded={showError}
                    className="eyebrow pressable shrink-0 rounded font-mono text-fg-faint hover:text-fg-muted"
                  >
                    {showError ? t('analyze.hideDetails') : t('analyze.details')}
                  </button>
                )}
              </div>
            )}
          </div>
        </div>

        <div className="flex shrink-0 flex-col justify-center gap-1">
          {(job.status === 'failed' || job.status === 'canceled') && (
            <IconButton
              icon={<RotateCw size={15} />}
              label={t('downloads.retry')}
              tone="accent"
              onClick={() => onRetry(job.id)}
            />
          )}
          {!isTerminal && (
            <IconButton
              icon={<X size={15} />}
              label={t('downloads.cancel')}
              tone="danger"
              onClick={() => onCancel(job.id)}
            />
          )}
        </div>
      </div>

      <AnimatePresence>
        {showError && job.error?.technical && (
          <motion.div
            variants={COLLAPSE}
            initial="initial"
            animate="animate"
            exit="exit"
            className="overflow-hidden"
          >
            <pre className="selectable max-h-32 overflow-auto whitespace-pre-wrap break-words border-t border-[var(--border)] bg-surface-sunken px-4 py-2.5 font-mono text-[11px] leading-relaxed text-fg-muted">
              {job.error.technical}
            </pre>
          </motion.div>
        )}
      </AnimatePresence>
    </motion.article>
  );
});

function extensionOf(name: string): string {
  const index = name.lastIndexOf('.');
  return index > 0 ? name.slice(index + 1) : '?';
}

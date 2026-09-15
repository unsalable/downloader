import { AnimatePresence, motion } from 'motion/react';
import {
  ArrowDown,
  ArrowUp,
  Check,
  FolderOpen,
  Pause,
  Play,
  RotateCw,
  SquareArrowOutUpRight,
  Trash2,
  X,
} from 'lucide-react';
import { memo, useState } from 'react';

import { Badge } from '@/components/ui/Badge';
import { IconButton } from '@/components/ui/IconButton';
import { PlatformBadge } from '@/components/ui/PlatformBadge';
import { Progress } from '@/components/ui/Progress';
import { Spinner } from '@/components/ui/Spinner';
import { useThumbnail } from '@/hooks/useThumbnail';
import { useTranslation } from '@/i18n';
import type { TranslationKey } from '@/i18n';
import { cn } from '@/lib/cn';
import { basename, clampPercent, formatBytes, formatEta, formatSpeed } from '@/lib/format';
import { IS_MOBILE, openFile, revealFile } from '@/lib/platform';
import type { DownloadTask } from '@/types';

interface DownloadCardProps {
  task: DownloadTask;
  onPause: (id: string) => void;
  onResume: (id: string) => void;
  onCancel: (id: string) => void;
  onRetry: (id: string) => void;
  onRemove: (id: string) => void;
  onMove: (id: string, delta: number) => void;
  reorderable: boolean;
}

const STATUS_TONE = {
  completed: 'success',
  failed: 'error',
  canceled: 'neutral',
  paused: 'warning',
} as const;

/** The colour of the rule down the left edge, which is how a row's state reads
 *  at a glance in a long queue. */
const EDGE = {
  completed: 'bg-[var(--success)]',
  failed: 'bg-[var(--error)]',
  canceled: 'bg-[var(--border-strong)]',
  paused: 'bg-[var(--warning)]',
  active: 'bg-[var(--accent)]',
  idle: 'bg-transparent',
} as const;

/**
 * One queue row.
 *
 * Memoised on the task object: progress events replace only the affected task,
 * so an active download re-renders its own card and leaves the rest alone.
 */
export const DownloadCard = memo(function DownloadCard({
  task,
  onPause,
  onResume,
  onCancel,
  onRetry,
  onRemove,
  onMove,
  reorderable,
}: DownloadCardProps) {
  const { t } = useTranslation();
  const { src } = useThumbnail(task.thumbnailUrl);
  const [showError, setShowError] = useState(false);

  const { progress, status } = task;
  const percent = clampPercent(progress.percent);
  const isActive = status === 'downloading' || status === 'preparing' || status === 'processing';
  const isDone = status === 'completed';
  const isTerminal = isDone || status === 'failed' || status === 'canceled';

  const stageLabel = t(`stage.${progress.stage}` as TranslationKey);
  const edge = isActive
    ? EDGE.active
    : status in EDGE
      ? EDGE[status as keyof typeof EDGE]
      : EDGE.idle;

  const errorTitle = task.error
    ? t(`error.${task.error.code}.title` as TranslationKey) === `error.${task.error.code}.title`
      ? task.error.title
      : t(`error.${task.error.code}.title` as TranslationKey)
    : '';

  return (
    <motion.article
      // See `Group` in DownloadsPage: no per-tick layout measuring on a phone.
      layout={IS_MOBILE ? false : 'position'}
      initial={{ opacity: 0, y: 8 }}
      animate={{ opacity: 1, y: 0 }}
      exit={{ opacity: 0, scale: 0.98, transition: { duration: 0.16 } }}
      transition={{ duration: 0.28, ease: [0.22, 1, 0.36, 1] }}
      className={cn(
        'group relative overflow-hidden rounded-[var(--radius-card)] border border-[var(--border)]',
        'bg-surface transition-colors duration-200 hover:border-[var(--border-strong)]',
      )}
    >
      {/* State rule. One pixel of colour carries what a coloured card would. */}
      <span aria-hidden="true" className={cn('absolute inset-y-0 left-0 w-[3px]', edge)} />

      <div className="flex gap-3 p-3 pl-4">
        <div className="relative aspect-video w-[100px] shrink-0 overflow-hidden rounded-[5px] bg-surface-sunken">
          {src ? (
            <img
              src={src}
              alt=""
              aria-hidden="true"
              draggable={false}
              className={cn(
                'no-drag size-full object-cover',
                // A finished or abandoned row stops competing for attention.
                isTerminal && !isDone && 'opacity-45 grayscale',
              )}
            />
          ) : (
            <div className="flex size-full items-center justify-center">
              <PlatformBadge platform={task.platform} size="md" />
            </div>
          )}
          {isDone && (
            <motion.div
              initial={{ opacity: 0 }}
              animate={{ opacity: 1 }}
              className="absolute inset-0 flex items-center justify-center bg-black/50"
            >
              <Check size={20} strokeWidth={2.5} className="text-white" />
            </motion.div>
          )}
        </div>

        <div className="flex min-w-0 flex-1 flex-col gap-1.5">
          <div className="flex items-start gap-2">
            <h3 className="line-clamp-1 flex-1 text-[13.5px] font-medium leading-snug text-fg">
              {task.title}
            </h3>

            <div className="reveal-on-hover flex shrink-0 items-center gap-0.5 transition-opacity duration-150">
              {reorderable && (
                <>
                  <IconButton
                    icon={<ArrowUp size={13} />}
                    label={t('downloads.moveUp')}
                    size="sm"
                    onClick={() => onMove(task.id, -1)}
                  />
                  <IconButton
                    icon={<ArrowDown size={13} />}
                    label={t('downloads.moveDown')}
                    size="sm"
                    onClick={() => onMove(task.id, 1)}
                  />
                </>
              )}
              <IconButton
                icon={<Trash2 size={13} />}
                label={t('downloads.remove')}
                size="sm"
                tone="danger"
                onClick={() => onRemove(task.id)}
              />
            </div>
          </div>

          {/* One meta line: format, state, and the stage while it is running. */}
          <div className="flex flex-wrap items-center gap-x-2 gap-y-1">
            <span className="metric text-[12px] text-fg-faint">{task.formatLabel}</span>
            {status in STATUS_TONE && (
              <Badge tone={STATUS_TONE[status as keyof typeof STATUS_TONE]}>
                {t(`downloads.${status === 'completed' ? 'complete' : status}` as TranslationKey)}
              </Badge>
            )}
            {status === 'queued' && <Badge tone="neutral">{t('downloads.waiting')}</Badge>}
            {isActive && (
              <span className="flex items-center gap-1.5 text-[11.5px] text-fg-muted">
                <Spinner size={11} />
                {stageLabel}
                {progress.stageCount > 1 && (
                  <span className="metric text-fg-faint">
                    {progress.stageIndex}/{progress.stageCount}
                  </span>
                )}
              </span>
            )}
          </div>

          <div className="mt-auto pt-1">
            {(isActive || status === 'paused') && (
              <>
                <div className="metric mb-1.5 flex items-center gap-3 text-[12px] text-fg-muted">
                  <span className="w-[42px] text-[15px] font-semibold text-fg">
                    {percent.toFixed(0)}%
                  </span>
                  <span>
                    {formatBytes(progress.receivedBytes)}
                    {progress.totalBytes != null && ` / ${formatBytes(progress.totalBytes)}`}
                  </span>
                  {status !== 'paused' && progress.speedBps > 0 && (
                    <>
                      <span className="ml-auto text-fg">{formatSpeed(progress.speedBps)}</span>
                      {progress.etaSec != null && (
                        <span className="w-[70px] text-right">
                          {t('downloads.eta', { time: formatEta(progress.etaSec) })}
                        </span>
                      )}
                    </>
                  )}
                </div>
                <Progress
                  value={progress.percent == null && isActive ? null : percent}
                  tone={status === 'paused' ? 'muted' : 'accent'}
                  label={task.title}
                />
              </>
            )}

            {isDone && task.outputPath && (
              <div className="flex items-center gap-2">
                {/* Beside two buttons a phone has room for a few letters of the
                    name, which the title above already gives in full. */}
                {!IS_MOBILE && (
                  <span className="metric min-w-0 flex-1 truncate text-[11.5px] text-fg-faint">
                    {basename(task.outputPath)}
                  </span>
                )}
                <button
                  type="button"
                  onClick={() => void openFile(task.outputPath!)}
                  className="flex shrink-0 items-center gap-1 rounded-[5px] px-1.5 py-1 text-[12px] font-medium text-accent transition-colors hover:bg-accent-soft"
                >
                  <SquareArrowOutUpRight size={12} />
                  {t('downloads.openFile')}
                </button>
                <button
                  type="button"
                  onClick={() => void revealFile(task.outputPath!)}
                  className="flex shrink-0 items-center gap-1 rounded-[5px] px-1.5 py-1 text-[12px] font-medium text-fg-muted transition-colors hover:bg-surface-hover hover:text-fg"
                >
                  <FolderOpen size={12} />
                  {t('downloads.openFolder')}
                </button>
              </div>
            )}

            {status === 'failed' && task.error && (
              <div className="flex items-baseline gap-2">
                <p className="min-w-0 flex-1 truncate text-[12.5px] leading-snug text-fg">
                  {errorTitle}
                </p>
                {task.error.technical && (
                  <button
                    type="button"
                    onClick={() => setShowError((value) => !value)}
                    className="eyebrow shrink-0 font-mono text-fg-faint transition-colors hover:text-fg-muted"
                  >
                    {showError ? t('analyze.hideDetails') : t('analyze.details')}
                  </button>
                )}
              </div>
            )}

            {status === 'canceled' && (
              <span className="text-[12px] text-fg-faint">{t('downloads.canceled')}</span>
            )}
          </div>
        </div>

        <div className="flex shrink-0 flex-col justify-center gap-1">
          {isActive && (
            <IconButton
              icon={<Pause size={15} />}
              label={t('downloads.pause')}
              onClick={() => onPause(task.id)}
            />
          )}
          {(status === 'paused' || status === 'queued') && (
            <IconButton
              icon={<Play size={15} />}
              label={t('downloads.resume')}
              tone="accent"
              onClick={() => onResume(task.id)}
            />
          )}
          {(status === 'failed' || status === 'canceled') && (
            <IconButton
              icon={<RotateCw size={15} />}
              label={t('downloads.retry')}
              tone="accent"
              onClick={() => onRetry(task.id)}
            />
          )}
          {!isTerminal && (
            <IconButton
              icon={<X size={15} />}
              label={t('downloads.cancel')}
              tone="danger"
              onClick={() => onCancel(task.id)}
            />
          )}
        </div>
      </div>

      <AnimatePresence>
        {showError && task.error?.technical && (
          <motion.div
            initial={{ opacity: 0, height: 0 }}
            animate={{ opacity: 1, height: 'auto' }}
            exit={{ opacity: 0, height: 0 }}
            transition={{ duration: 0.2 }}
            className="overflow-hidden"
          >
            <pre className="selectable max-h-32 overflow-auto whitespace-pre-wrap break-words border-t border-[var(--border)] bg-surface-sunken px-4 py-2.5 font-mono text-[11px] leading-relaxed text-fg-muted">
              {task.error.technical}
            </pre>
          </motion.div>
        )}
      </AnimatePresence>
    </motion.article>
  );
});

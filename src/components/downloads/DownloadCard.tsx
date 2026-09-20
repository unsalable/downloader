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
import { COLLAPSE, LIST_ITEM, SPRING, T } from '@/lib/motion';
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

  // Two clusters, placed differently on a pointer and on a screen. What a row
  // *is* -- reorder it, forget it -- sits by the title on the desktop and is
  // revealed by the pointer; what a row is *doing* -- pause, cancel, retry --
  // sits in a column at the end, always visible. A phone has no pointer to
  // reveal anything with and no width to spare, so both go to one bar under
  // the row, the way a History entry's do.
  const rowActions = (
    <>
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
    </>
  );

  const stateActions = (
    <>
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
    </>
  );

  return (
    <motion.article
      // See `Group` in DownloadsPage: no per-tick layout measuring on a phone.
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
      {/* State rule. One pixel of colour carries what a coloured card would. */}
      <span
        aria-hidden="true"
        className={cn(
          'absolute inset-y-0 left-0 w-[3px] transition-colors duration-250 ease-out-quint',
          edge,
        )}
      />

      {/* A phone's action bar is a full-width row under the thumbnail rather
          than a third column, so it wraps. */}
      <div className={cn('flex gap-3 p-3 pl-4', IS_MOBILE && 'flex-wrap gap-y-0')}>
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
            // Finishing is the one moment in a row's life worth marking. The
            // scrim fades, the mark lands with a little weight, and then it is
            // over -- long enough to be seen, short enough to not be waited on.
            <motion.div
              initial={{ opacity: 0 }}
              animate={{ opacity: 1 }}
              transition={T.component}
              className="absolute inset-0 flex items-center justify-center bg-black/50"
            >
              <motion.span
                initial={{ scale: 0.55, opacity: 0 }}
                animate={{ scale: 1, opacity: 1 }}
                transition={SPRING.snap}
              >
                <Check size={20} strokeWidth={2.5} className="text-white" />
              </motion.span>
            </motion.div>
          )}
        </div>

        <div className="flex min-w-0 flex-1 flex-col gap-1.5">
          <div className="flex items-start gap-2">
            <h3 className="line-clamp-1 min-w-0 flex-1 text-[13.5px] font-medium leading-snug text-fg">
              {task.title}
            </h3>

            {/* On a phone these live in the bar at the foot of the row. A
                pointer can be waited for, so here they stay hidden until it
                arrives and cost the title nothing; a fingertip has no hover,
                so on touch they would be permanently parked on the title's
                line -- and on a 375px screen that left the title two
                characters wide. */}
            {!IS_MOBILE && (
              <div className="reveal-on-hover flex shrink-0 items-center gap-0.5 transition-opacity duration-150 ease-out-quint">
                {rowActions}
              </div>
            )}
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
                  className="pressable flex shrink-0 items-center gap-1 rounded-[5px] px-1.5 py-1 text-[12px] font-medium text-accent hover:bg-accent-soft"
                >
                  <SquareArrowOutUpRight size={12} />
                  {t('downloads.openFile')}
                </button>
                <button
                  type="button"
                  onClick={() => void revealFile(task.outputPath!)}
                  className="pressable flex shrink-0 items-center gap-1 rounded-[5px] px-1.5 py-1 text-[12px] font-medium text-fg-muted hover:bg-surface-hover hover:text-fg"
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
                    aria-expanded={showError}
                    className="eyebrow pressable shrink-0 rounded font-mono text-fg-faint hover:text-fg-muted"
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

        {IS_MOBILE ? (
          <div className="mt-2.5 flex w-full items-center justify-end gap-0.5 border-t border-[var(--border)] pt-1">
            {rowActions}
            {stateActions}
          </div>
        ) : (
          <div className="flex shrink-0 flex-col justify-center gap-1">{stateActions}</div>
        )}
      </div>

      <AnimatePresence>
        {showError && task.error?.technical && (
          <motion.div
            variants={COLLAPSE}
            initial="initial"
            animate="animate"
            exit="exit"
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

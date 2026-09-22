import { AnimatePresence, motion } from 'motion/react';
import { FolderOpen, Pause, Play, RotateCw, X } from 'lucide-react';
import { memo, useState } from 'react';

import { IconButton } from '@/components/ui/IconButton';
import { ROW_LINE } from '@/components/ui/ListGroup';
import { PlatformBadge } from '@/components/ui/PlatformBadge';
import { Progress } from '@/components/ui/Progress';
import { useThumbnail } from '@/hooks/useThumbnail';
import { useTranslation } from '@/i18n';
import type { TranslationKey } from '@/i18n';
import { cn } from '@/lib/cn';
import { clampPercent, formatBytes, formatEta, formatSpeed } from '@/lib/format';
import { COLLAPSE, LIST_ITEM } from '@/lib/motion';
import { IS_MOBILE, openFile, revealFile } from '@/lib/platform';
import type { DownloadStage, DownloadTask } from '@/types';

interface DownloadCardProps {
  task: DownloadTask;
  onPause: (id: string) => void;
  onResume: (id: string) => void;
  onCancel: (id: string) => void;
  onRetry: (id: string) => void;
  onRemove: (id: string) => void;
}

/** Stages that are simply "downloading"; the bar already says that. */
const TRANSFER_STAGES: ReadonlySet<DownloadStage> = new Set(['video', 'audio', 'image']);

/** 30px under a pointer, 36px under a fingertip. */
const ACTION_SIZE = IS_MOBILE ? 'sm' : 'md';

const LINE = cn(ROW_LINE, 'gap-x-3');

/**
 * One row of the downloads list: thumbnail, title, a single line saying what
 * state it is in, and the actions that state allows.
 *
 * Memoised on the task object: progress events replace only the affected task,
 * so an active download re-renders its own row and leaves the rest alone.
 */
export const DownloadCard = memo(function DownloadCard({
  task,
  onPause,
  onResume,
  onCancel,
  onRetry,
  onRemove,
}: DownloadCardProps) {
  const { t } = useTranslation();
  const { src } = useThumbnail(task.thumbnailUrl);
  const [showError, setShowError] = useState(false);
  // A finished row is a button that opens its file, which is a big target to
  // press for nothing when the file has since been moved or deleted. The
  // attempt is what finds that out, so the row says so afterwards.
  const [missing, setMissing] = useState(false);

  const { progress, status } = task;
  const percent = clampPercent(progress.percent);
  const isActive = status === 'downloading' || status === 'preparing' || status === 'processing';
  const isDone = status === 'completed';
  const isAbandoned = status === 'failed' || status === 'canceled';
  const outputPath = isDone ? task.outputPath : null;

  const errorTitle = task.error
    ? t(`error.${task.error.code}.title` as TranslationKey) === `error.${task.error.code}.title`
      ? task.error.title
      : t(`error.${task.error.code}.title` as TranslationKey)
    : t('downloads.failed');
  const technical = status === 'failed' ? task.error?.technical : null;

  // A finished row's thumbnail and text sit inside a button, which may only
  // hold phrasing content; every other row is free to hold the bar and the
  // error line, which are blocks.
  const Box = outputPath ? 'span' : 'div';

  const main = (
    <>
      <Box
        className={cn(
          'flex h-11 w-[72px] shrink-0 items-center justify-center overflow-hidden',
          'rounded-[var(--radius-thumb)] bg-surface-sunken',
          isAbandoned && 'opacity-60',
        )}
      >
        {src ? (
          <img
            src={src}
            alt=""
            aria-hidden="true"
            draggable={false}
            className="no-drag size-full object-cover"
          />
        ) : (
          <PlatformBadge platform={task.platform} size="md" />
        )}
      </Box>

      <Box className="block min-w-0 flex-1">
        <span className="block truncate text-[13.5px] font-medium leading-[18px] text-fg">
          {task.title}
        </span>

        {isActive && (
          <>
            <Progress
              value={progress.percent == null ? null : percent}
              label={task.title}
              className="mt-1.5"
            />
            <span className={cn(LINE, 'mt-1')}>
              {/* Held to a width so the values after it do not shuffle sideways
                  every time a digit is added. */}
              {progress.percent != null && (
                <span className="min-w-[34px]">{percent.toFixed(0)}%</span>
              )}
              {!TRANSFER_STAGES.has(progress.stage) && (
                <span className="min-w-0 max-w-full truncate">
                  {t(`stage.${progress.stage}` as TranslationKey)}
                </span>
              )}
              {progress.speedBps > 0 && (
                <>
                  <span className={cn(!IS_MOBILE && 'min-w-[64px]')}>
                    {formatSpeed(progress.speedBps)}
                  </span>
                  {progress.etaSec != null && (
                    // A phone has room for the time but not for the word.
                    <span>
                      {IS_MOBILE
                        ? formatEta(progress.etaSec)
                        : t('downloads.eta', { time: formatEta(progress.etaSec) })}
                    </span>
                  )}
                </>
              )}
            </span>
          </>
        )}

        {status === 'queued' && <span className={cn(LINE, 'mt-0.5')}>{t('downloads.queued')}</span>}

        {status === 'paused' && (
          <span className={cn(LINE, 'mt-0.5')}>
            <span>{t('downloads.paused')}</span>
            {progress.percent != null && <span>{percent.toFixed(0)}%</span>}
            {progress.receivedBytes > 0 && (
              <span>
                {formatBytes(progress.receivedBytes)}
                {progress.totalBytes != null && ` / ${formatBytes(progress.totalBytes)}`}
              </span>
            )}
          </span>
        )}

        {isDone && (
          <span className={cn(LINE, 'mt-0.5')}>
            {missing ? (
              <span className="min-w-0 max-w-full truncate text-error">{t('file.missing')}</span>
            ) : (
              <>
                <span className="min-w-0 max-w-full truncate">{task.formatLabel}</span>
                {progress.totalBytes != null && progress.totalBytes > 0 && (
                  <span>{formatBytes(progress.totalBytes)}</span>
                )}
              </>
            )}
          </span>
        )}

        {status === 'failed' && (
          <div className="mt-0.5 flex items-baseline gap-2 text-[12.5px] leading-[18px]">
            <p className="min-w-0 truncate text-error">{errorTitle}</p>
            {technical && (
              <button
                type="button"
                onClick={() => setShowError((value) => !value)}
                aria-expanded={showError}
                // The padding is only there to be pressed; the negative margin
                // keeps it from making the row any taller.
                className="-my-2 shrink-0 rounded-[6px] py-2 text-fg-muted transition-colors duration-150 ease-out-quint hover:text-fg"
              >
                {t('downloads.details')}
              </button>
            )}
          </div>
        )}

        {status === 'canceled' && (
          <span className={cn(LINE, 'mt-0.5')}>{t('downloads.canceled')}</span>
        )}
      </Box>
    </>
  );

  // The thumbnail and the text are one target, and the padding on the row's
  // left is inside it, so the whole row up to the actions opens the file.
  const mainClass = cn(
    'flex min-w-0 flex-1 items-center py-2.5 pr-2',
    IS_MOBILE ? 'gap-3 pl-3' : 'gap-4 pl-4',
  );

  return (
    <motion.article
      // Layout animation measures the row on every render, and progress renders
      // it several times a second; a phone skips the glide.
      layout={IS_MOBILE ? false : 'position'}
      variants={LIST_ITEM}
      initial="initial"
      animate="animate"
      exit="exit"
      aria-label={task.title}
      className={cn(
        'group',
        outputPath &&
          'transition-colors duration-150 ease-out-quint hover:bg-surface-hover has-[[data-open]:active]:bg-surface-active',
      )}
    >
      <div className={cn('flex items-center', IS_MOBILE ? 'pr-1.5' : 'pr-3')}>
        {outputPath ? (
          <button
            type="button"
            data-open=""
            onClick={() => void openFile(outputPath).catch(() => setMissing(true))}
            aria-label={t('downloads.openFileNamed', { title: task.title })}
            className={cn(mainClass, 'cursor-pointer rounded-[12px] text-left')}
          >
            {main}
          </button>
        ) : (
          <div className={mainClass}>{main}</div>
        )}

        <div
          className={cn(
            'flex shrink-0 items-center gap-0.5',
            // A row that is still doing something keeps its controls in view:
            // they belong to what is happening. A finished one is a record,
            // and an icon parked at the far end of it reads as unattached --
            // a short title leaves it stranded in the gap -- so those wait for
            // the pointer that is going to use them. A touch screen has no
            // pointer to wait for, so there they are always shown.
            (isDone || isAbandoned) &&
              'reveal-on-hover transition-opacity duration-150 ease-out-quint group-focus-within:opacity-100',
          )}
        >
          {isActive && (
            <IconButton
              icon={<Pause size={15} />}
              label={t('downloads.pause')}
              size={ACTION_SIZE}
              onClick={() => onPause(task.id)}
            />
          )}
          {(status === 'paused' || status === 'queued') && (
            <IconButton
              icon={<Play size={15} />}
              label={t('downloads.resume')}
              size={ACTION_SIZE}
              onClick={() => onResume(task.id)}
            />
          )}
          {isAbandoned && (
            <IconButton
              icon={<RotateCw size={15} />}
              label={t('downloads.retry')}
              size={ACTION_SIZE}
              onClick={() => onRetry(task.id)}
            />
          )}
          {outputPath && (
            <IconButton
              icon={<FolderOpen size={15} />}
              label={t('downloads.showInFolder')}
              size={ACTION_SIZE}
              onClick={() => void revealFile(outputPath).catch(() => setMissing(true))}
            />
          )}
          {isDone || isAbandoned ? (
            <IconButton
              icon={<X size={15} />}
              label={t('downloads.remove')}
              size={ACTION_SIZE}
              onClick={() => onRemove(task.id)}
            />
          ) : (
            <IconButton
              icon={<X size={15} />}
              label={t('downloads.cancel')}
              size={ACTION_SIZE}
              onClick={() => onCancel(task.id)}
            />
          )}
        </div>
      </div>

      <AnimatePresence initial={false}>
        {showError && technical && (
          <motion.div
            variants={COLLAPSE}
            initial="initial"
            animate="animate"
            exit="exit"
            className="overflow-hidden"
          >
            <pre className="selectable max-h-32 overflow-auto whitespace-pre-wrap break-words bg-surface-sunken px-4 py-2.5 font-mono text-[12px] leading-relaxed text-fg-muted">
              {technical}
            </pre>
          </motion.div>
        )}
      </AnimatePresence>
    </motion.article>
  );
});

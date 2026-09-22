import { AnimatePresence, motion } from 'motion/react';
import { FileAudio, FileVideo, FolderOpen, RotateCw, X } from 'lucide-react';
import { memo, useState } from 'react';

import { IconButton } from '@/components/ui/IconButton';
import { ROW_LINE } from '@/components/ui/ListGroup';
import { Progress } from '@/components/ui/Progress';
import { useTranslation } from '@/i18n';
import type { TranslationKey } from '@/i18n';
import { cn } from '@/lib/cn';
import { clampPercent, formatBytes } from '@/lib/format';
import { COLLAPSE, LIST_ITEM } from '@/lib/motion';
import { IS_MOBILE, openFile, revealFile } from '@/lib/platform';
import type { ConvertJob } from '@/types';

interface ConvertCardProps {
  job: ConvertJob;
  onCancel: (id: string) => void;
  onRetry: (id: string) => void;
  onRemove: (id: string) => void;
}

/** 30px under a pointer, 36px under a fingertip. */
const ACTION_SIZE = IS_MOBILE ? 'sm' : 'md';

const LINE = cn(ROW_LINE, 'gap-x-3');

/**
 * One row of the conversion list, in the same anatomy as a download's: a tile,
 * the file's name, a single line saying what state it is in, and the actions
 * that state allows.
 *
 * Memoised on the job: progress replaces only the running job, so an encode in
 * flight re-renders its own row and leaves the rest alone.
 */
export const ConvertCard = memo(function ConvertCard({
  job,
  onCancel,
  onRetry,
  onRemove,
}: ConvertCardProps) {
  const { t } = useTranslation();
  const [showError, setShowError] = useState(false);

  const { status } = job;
  const percent = clampPercent(job.percent);
  const isDone = status === 'completed';
  const isAbandoned = status === 'failed' || status === 'canceled';
  const outputPath = isDone ? job.outputPath : null;
  const formats = describeFormats(job);
  const Icon = job.kind === 'audio' ? FileAudio : FileVideo;

  const errorTitle = job.error
    ? t(`error.${job.error.code}.title` as TranslationKey) === `error.${job.error.code}.title`
      ? job.error.title
      : t(`error.${job.error.code}.title` as TranslationKey)
    : t('convert.status.failed');
  const technical = status === 'failed' ? job.error?.technical : null;

  // A finished row's tile and text sit inside a button, which may only hold
  // phrasing content; every other row is free to hold the bar and the error
  // line, which are blocks.
  const Box = outputPath ? 'span' : 'div';

  const main = (
    <>
      <Box
        className={cn(
          'flex size-11 shrink-0 items-center justify-center',
          'rounded-[var(--radius-thumb)] bg-surface-sunken text-fg-muted',
          isAbandoned && 'opacity-60',
        )}
      >
        <Icon size={19} aria-hidden="true" />
      </Box>

      <Box className="block min-w-0 flex-1">
        <span className="block truncate text-[13.5px] font-medium leading-[18px] text-fg">
          {job.inputName}
        </span>

        {status === 'running' && (
          <>
            <Progress
              value={job.percent == null ? null : percent}
              label={job.inputName}
              className="mt-1.5"
            />
            <span className={cn(LINE, 'mt-1')}>
              {/* Held to a width so the format after it does not step sideways
                  every time a digit is added. Without a duration there is no
                  figure to show, and the word stands in for it. */}
              {job.percent == null ? (
                <span>{t('convert.working')}</span>
              ) : (
                <span className="min-w-[34px]">{percent.toFixed(0)}%</span>
              )}
              <span>{formats}</span>
            </span>
          </>
        )}

        {status === 'queued' && (
          <span className={cn(LINE, 'mt-0.5')}>{t('convert.status.queued')}</span>
        )}

        {isDone && (
          <span className={cn(LINE, 'mt-0.5')}>
            <span>{formats}</span>
            {job.outputSizeBytes != null && <span>{formatBytes(job.outputSizeBytes)}</span>}
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
          <span className={cn(LINE, 'mt-0.5')}>{t('convert.status.canceled')}</span>
        )}
      </Box>
    </>
  );

  // The tile and the text are one target, and the padding on the row's left is
  // inside it, so the whole row up to the actions opens the file.
  const mainClass = 'flex min-w-0 flex-1 items-center gap-3 py-2.5 pl-4 pr-2';

  return (
    <motion.article
      // Layout animation measures the row on every render, and progress renders
      // it several times a second; a phone skips the glide.
      layout={IS_MOBILE ? false : 'position'}
      variants={LIST_ITEM}
      initial="initial"
      animate="animate"
      exit="exit"
      aria-label={job.inputName}
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
            onClick={() => void openFile(outputPath)}
            aria-label={t('downloads.openFileNamed', { title: job.inputName })}
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
            // See DownloadCard: a running job keeps its controls, a finished
            // one hands them back to the pointer rather than parking an icon
            // at the far end of the row with nothing tying it to the text.
            (isDone || isAbandoned) &&
              'reveal-on-hover transition-opacity duration-150 ease-out-quint group-focus-within:opacity-100',
          )}
        >
          {isAbandoned && (
            <IconButton
              icon={<RotateCw size={15} />}
              label={t('downloads.retry')}
              size={ACTION_SIZE}
              onClick={() => onRetry(job.id)}
            />
          )}
          {outputPath && (
            <IconButton
              icon={<FolderOpen size={15} />}
              label={t('downloads.showInFolder')}
              size={ACTION_SIZE}
              onClick={() => void revealFile(outputPath)}
            />
          )}
          {isDone || isAbandoned ? (
            <IconButton
              icon={<X size={15} />}
              label={t('convert.remove')}
              size={ACTION_SIZE}
              onClick={() => onRemove(job.id)}
            />
          ) : (
            <IconButton
              icon={<X size={15} />}
              label={t('downloads.cancel')}
              size={ACTION_SIZE}
              onClick={() => onCancel(job.id)}
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

/**
 * "MKV → MP4". Upper-cased here rather than in CSS: under `lang="tr"` the
 * browser's `uppercase` turns "avi" into "AVİ".
 */
function describeFormats(job: ConvertJob): string {
  const target = job.options.targetFormat.toUpperCase();
  const dot = job.inputName.lastIndexOf('.');
  if (dot <= 0) return target;
  return `${job.inputName.slice(dot + 1).toUpperCase()} → ${target}`;
}

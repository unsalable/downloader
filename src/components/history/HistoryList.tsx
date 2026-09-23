import { AnimatePresence, motion } from 'motion/react';
import { Check, Clock, Download, FolderOpen, Search, Trash2, X } from 'lucide-react';
import { memo, useCallback, useEffect, useState } from 'react';

import { Button } from '@/components/ui/Button';
import { EmptyState } from '@/components/ui/EmptyState';
import { IconButton } from '@/components/ui/IconButton';
import { InlineNotice } from '@/components/ui/InlineNotice';
import { ListGroup, ROW_LINE } from '@/components/ui/ListGroup';
import { Modal } from '@/components/ui/Modal';
import { PageHeader } from '@/components/ui/PageHeader';
import { PlatformBadge } from '@/components/ui/PlatformBadge';
import { TextInput } from '@/components/ui/TextInput';
import { useDebouncedValue } from '@/hooks/useDebouncedValue';
import { useMomentary } from '@/hooks/useMomentary';
import { useThumbnail } from '@/hooks/useThumbnail';
import { useTranslation } from '@/i18n';
import type { TranslationKey } from '@/i18n';
import { cn } from '@/lib/cn';
import { formatBytes, formatDate, formatDay } from '@/lib/format';
import { LIST_ITEM } from '@/lib/motion';
import { IS_MOBILE, openFile, revealFile } from '@/lib/platform';
import * as ipc from '@/services/ipc';
import type { AppErrorInfo, HistoryEntry } from '@/types';

const PAGE_SIZE = 60;

/** Where a row's text starts: its padding, the 72px thumbnail, the gap. */
const TEXT_INSET = IS_MOBILE ? 96 : 104;

/** 30px under a pointer, 36px under a fingertip. */
const ACTION_SIZE = IS_MOBILE ? 'sm' : 'md';

/**
 * How a removed row leaves. The desktop lifts it out of the flow, so the rows
 * under it glide up while it fades. A phone skips the glide (see the row's
 * `layout`), and lifted out there the row would fade over the one that jumped
 * into its place -- so it fades where it stands, and the gap closes after.
 */
const LEAVING = IS_MOBILE ? 'sync' : 'popLayout';

/**
 * Queue an entry again. Resolves to the failure, if there was one, so the row
 * that asked can say so itself.
 */
async function redownload(entry: HistoryEntry): Promise<AppErrorInfo | null> {
  // Re-run the exact request that produced this file when it was recorded;
  // otherwise fall back to a best-quality request for the same URL.
  const request = entry.request ?? {
    url: entry.url,
    mode: 'video' as const,
    quality: { type: 'best' as const },
    videoFormatId: null,
    audioFormatId: null,
    container: null,
    watermark: 'any' as const,
    outputDir: null,
    title: entry.title,
    thumbnailUrl: entry.thumbnailUrl,
    platform: entry.platform,
  };

  try {
    await ipc.enqueueDownload(request);
    return null;
  } catch (error) {
    return ipc.toAppError(error);
  }
}

/**
 * The history itself: its search, its rows, what can be done to them, and what
 * it says when there are none. The desktop gives it a screen of its own (see
 * `HistoryPage`), where the search and the clear button share the screen's
 * title bar; a phone shows it as the second half of Downloads, under the
 * control that switches to it, so there it brings no title.
 */
export function HistoryList({ onGoHome }: { onGoHome: () => void }) {
  const { t } = useTranslation();

  const [entries, setEntries] = useState<HistoryEntry[]>([]);
  const [total, setTotal] = useState(0);
  const [query, setQuery] = useState('');
  const [loading, setLoading] = useState(true);
  const [confirmClear, setConfirmClear] = useState(false);

  const debouncedQuery = useDebouncedValue(query, 220);

  const refresh = useCallback(async () => {
    setLoading(true);
    try {
      const [rows, count] = await Promise.all([
        ipc.listHistory(debouncedQuery || undefined, PAGE_SIZE, 0),
        ipc.countHistory(),
      ]);
      setEntries(rows);
      setTotal(count);
    } finally {
      setLoading(false);
    }
  }, [debouncedQuery]);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  // Stable, so typing in the search field does not re-render every row.
  const remove = useCallback(async (id: number) => {
    await ipc.deleteHistoryEntry(id);
    setEntries((rows) => rows.filter((row) => row.id !== id));
    setTotal((value) => Math.max(0, value - 1));
  }, []);

  const clearAll = async () => {
    await ipc.clearHistory();
    setConfirmClear(false);
    void refresh();
  };

  const isEmpty = !loading && entries.length === 0;

  const search = (
    <TextInput
      value={query}
      onChange={(event) => setQuery(event.target.value)}
      placeholder={t('history.search')}
      icon={<Search size={15} />}
      aria-label={t('history.search')}
    />
  );

  return (
    <>
      {IS_MOBILE ? (
        // The control above already says this is the history; the field gets
        // the line.
        <div className="flex items-center gap-1.5 pb-3">
          <div className="min-w-0 flex-1">{search}</div>
          {total > 0 && (
            <IconButton
              icon={<Trash2 size={16} />}
              label={t('history.clearAll')}
              onClick={() => setConfirmClear(true)}
            />
          )}
        </div>
      ) : (
        <PageHeader
          title={t('history.title')}
          actions={
            <>
              <div className="w-[240px]">{search}</div>
              {total > 0 && (
                <Button size="sm" variant="ghost" onClick={() => setConfirmClear(true)}>
                  {t('history.clearAll')}
                </Button>
              )}
            </>
          }
        />
      )}

      {isEmpty && (
        <EmptyState
          icon={debouncedQuery ? Search : Clock}
          title={debouncedQuery ? t('history.noResults', { query: debouncedQuery }) : t('history.emptyTitle')}
          body={debouncedQuery ? undefined : t('history.emptyBody')}
          action={
            debouncedQuery ? (
              <Button variant="secondary" onClick={() => setQuery('')}>
                {t('input.clear')}
              </Button>
            ) : (
              <Button variant="secondary" onClick={onGoHome}>
                {t('downloads.goHome')}
              </Button>
            )
          }
        />
      )}

      {entries.length > 0 && (
        <ListGroup inset={TEXT_INSET} className="relative">
          <AnimatePresence initial={false} mode={LEAVING}>
            {entries.map((entry) => (
              <HistoryRow key={entry.id} entry={entry} onRemove={remove} />
            ))}
          </AnimatePresence>
        </ListGroup>
      )}

      <Modal
        open={confirmClear}
        onClose={() => setConfirmClear(false)}
        title={t('history.clearConfirm')}
        description={t('history.clearConfirmBody')}
        closeLabel={t('common.close')}
        footer={
          <>
            <Button variant="ghost" onClick={() => setConfirmClear(false)}>
              {t('common.cancel')}
            </Button>
            <Button variant="danger" onClick={() => void clearAll()} data-autofocus>
              {t('history.clearAll')}
            </Button>
          </>
        }
      />
    </>
  );
}

const LINE = cn(ROW_LINE, 'mt-0.5 gap-x-3');

/**
 * The same row the Downloads list is made of: a file that is still there opens
 * when the row is pressed, and the second line carries everything else.
 */
const HistoryRow = memo(function HistoryRow({
  entry,
  onRemove,
}: {
  entry: HistoryEntry;
  onRemove: (id: number) => void;
}) {
  const { t, language } = useTranslation();
  const { src } = useThumbnail(entry.thumbnailUrl);

  const [busy, setBusy] = useState(false);
  const [requeued, markRequeued] = useMomentary();
  const [failure, setFailure] = useState<AppErrorInfo | null>(null);
  const [gone, setGone] = useState(false);

  // Nothing pops up to say the entry was queued or was not. The button turns
  // into a check for a moment, and a failure is written into the row.
  const handleRedownload = async () => {
    setBusy(true);
    setFailure(null);
    const error = await redownload(entry);
    setBusy(false);
    if (error) {
      setFailure(error);
      return;
    }
    markRequeued();
  };

  const failureTitle = failure
    ? t(`error.${failure.code}.title` as TranslationKey) === `error.${failure.code}.title`
      ? failure.title
      : t(`error.${failure.code}.title` as TranslationKey)
    : '';

  // The backend said whether the file was there when the page was read. It can
  // have gone since, and pressing the row is what finds that out, so a refusal
  // to open it moves the row to the state it should already have been in.
  const exists = entry.fileExists && !gone;

  // Inside a button only phrasing content is allowed.
  const Box = exists ? 'span' : 'div';

  const main = (
    <>
      <Box
        className={cn(
          'flex h-11 w-[72px] shrink-0 items-center justify-center overflow-hidden',
          'rounded-[var(--radius-thumb)] bg-surface-sunken',
          !exists && 'opacity-60',
        )}
      >
        {src ? (
          <img src={src} alt="" aria-hidden="true" draggable={false} className="no-drag size-full object-cover" />
        ) : (
          <PlatformBadge platform={entry.platform} size="md" />
        )}
      </Box>

      <Box className="block min-w-0 flex-1">
        <span className="block truncate text-[13.5px] font-medium leading-[18px] text-fg">
          {entry.title}
        </span>
        {exists ? (
          <span className={LINE}>
            {/* A phone has room for the day, not for the year and the minute. */}
            <span>
              {IS_MOBILE
                ? formatDay(entry.createdAt, language)
                : formatDate(entry.createdAt, language)}
            </span>
            <span>{entry.qualityLabel}</span>
            <span>{entry.container.toUpperCase()}</span>
            {entry.fileSize != null && <span>{formatBytes(entry.fileSize)}</span>}
          </span>
        ) : (
          <span className={LINE}>
            <span className="min-w-0 max-w-full truncate">{t('file.missing')}</span>
          </span>
        )}
      </Box>
    </>
  );

  const mainClass = cn(
    'flex min-w-0 flex-1 items-center py-2.5 pr-2',
    IS_MOBILE ? 'gap-3 pl-3' : 'gap-4 pl-4',
  );

  const redownloadButton = (
    <IconButton
      icon={
        requeued ? <Check size={15} className="text-success" /> : <Download size={15} />
      }
      label={t('history.redownload')}
      size={ACTION_SIZE}
      disabled={busy}
      onClick={() => void handleRedownload()}
    />
  );

  const removeButton = (
    <IconButton
      icon={<X size={15} />}
      label={t('history.delete')}
      size={ACTION_SIZE}
      onClick={() => onRemove(entry.id)}
    />
  );

  // A row at rest is the record and nothing else. An icon parked at the far
  // end of every row reads as unattached to it -- a short title leaves it
  // stranded in the gap -- so the actions wait for the pointer that is going
  // to use them. They stay in view while the check mark is showing, so the
  // answer to a click does not fade out with the pointer, and a touch screen
  // has no pointer to wait for, so there they are simply always there.
  const quiet = cn(
    'reveal-on-hover flex shrink-0 items-center gap-0.5 transition-opacity duration-150 ease-out-quint',
    'group-focus-within:opacity-100',
    requeued && 'opacity-100!',
  );

  return (
    <motion.article
      // A layout animation measures every row in the list whenever one of them
      // changes; a phone skips the glide, as the Downloads rows do.
      layout={IS_MOBILE ? false : 'position'}
      variants={LIST_ITEM}
      initial="initial"
      animate="animate"
      exit="exit"
      aria-label={entry.title}
      className={cn(
        'group',
        exists &&
          'transition-colors duration-150 ease-out-quint hover:bg-surface-hover has-[[data-open]:active]:bg-surface-active',
      )}
    >
      <div className={cn('flex items-center', IS_MOBILE ? 'pr-1.5' : 'pr-3')}>
        {exists ? (
          <button
            type="button"
            data-open=""
            onClick={() => void openFile(entry.filePath).catch(() => setGone(true))}
            aria-label={t('downloads.openFileNamed', { title: entry.title })}
            className={cn(mainClass, 'cursor-pointer rounded-[12px] text-left')}
          >
            {main}
          </button>
        ) : (
          <div className={mainClass}>{main}</div>
        )}

        <div className={quiet}>
          {/* On a phone this only opens the system's Downloads view, and
              pressing the row already opens the file; a third button would
              leave the title a few letters wide. */}
          {exists && !IS_MOBILE && (
            <IconButton
              icon={<FolderOpen size={15} />}
              label={t('downloads.showInFolder')}
              size={ACTION_SIZE}
              onClick={() => void revealFile(entry.filePath).catch(() => setGone(true))}
            />
          )}
          {redownloadButton}
          {removeButton}
        </div>
      </div>

      {failure && (
        <div className="pb-2.5 pr-4" style={{ paddingLeft: TEXT_INSET }}>
          <InlineNotice tone="error">
            {failureTitle}. {failure.message}
          </InlineNotice>
        </div>
      )}

      <span role="status" className="sr-only">
        {requeued ? t('history.requeued') : ''}
      </span>
    </motion.article>
  );
});

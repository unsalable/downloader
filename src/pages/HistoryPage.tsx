import { AnimatePresence, motion } from 'motion/react';
import { openPath, revealItemInDir } from '@tauri-apps/plugin-opener';
import {
  Download,
  FileWarning,
  FolderOpen,
  Search,
  SquareArrowOutUpRight,
  Trash2,
} from 'lucide-react';
import { memo, useCallback, useEffect, useState } from 'react';

import { Badge } from '@/components/ui/Badge';
import { Button } from '@/components/ui/Button';
import { EmptyState, HistoryIllustration } from '@/components/ui/EmptyState';
import { IconButton } from '@/components/ui/IconButton';
import { Modal } from '@/components/ui/Modal';
import { PlatformBadge } from '@/components/ui/PlatformBadge';
import { TextInput } from '@/components/ui/TextInput';
import { useDebouncedValue } from '@/hooks/useDebouncedValue';
import { useThumbnail } from '@/hooks/useThumbnail';
import { useTranslation } from '@/i18n';
import { cn } from '@/lib/cn';
import { basename, formatBytes, formatDate } from '@/lib/format';
import * as ipc from '@/services/ipc';
import { useToastStore } from '@/stores/useToastStore';
import type { HistoryEntry } from '@/types';

const PAGE_SIZE = 60;

export function HistoryPage({ onGoHome }: { onGoHome: () => void }) {
  const { t } = useTranslation();
  const pushToast = useToastStore((state) => state.push);

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

  const remove = async (id: number) => {
    await ipc.deleteHistoryEntry(id);
    setEntries((rows) => rows.filter((row) => row.id !== id));
    setTotal((value) => Math.max(0, value - 1));
  };

  const redownload = async (entry: HistoryEntry) => {
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
      pushToast({ tone: 'success', title: t('action.addedToQueue'), body: entry.title });
    } catch (error) {
      const info = ipc.toAppError(error);
      pushToast({ tone: 'error', title: info.title, body: info.message });
    }
  };

  const clearAll = async () => {
    await ipc.clearHistory();
    setConfirmClear(false);
    void refresh();
  };

  const isEmpty = !loading && entries.length === 0;

  return (
    <div className="mx-auto w-full max-w-[820px] px-6 pb-12">
      <div className="sticky top-0 z-10 -mx-6 flex items-center gap-3 bg-bg/85 px-6 py-3 backdrop-blur-xl">
        <div className="w-[260px]">
          <TextInput
            value={query}
            onChange={(event) => setQuery(event.target.value)}
            placeholder={t('history.search')}
            icon={<Search size={15} />}
            aria-label={t('history.search')}
          />
        </div>
        <span className="tabular text-[12.5px] text-fg-faint">
          {t('history.entries', { n: total })}
        </span>
        {total > 0 && (
          <Button
            size="sm"
            variant="ghost"
            icon={<Trash2 size={14} />}
            className="ml-auto"
            onClick={() => setConfirmClear(true)}
          >
            {t('history.clearAll')}
          </Button>
        )}
      </div>

      {isEmpty && (
        <EmptyState
          illustration={<HistoryIllustration />}
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

      <div className="flex flex-col gap-2 pt-1">
        <AnimatePresence initial={false} mode="popLayout">
          {entries.map((entry) => (
            <HistoryRow
              key={entry.id}
              entry={entry}
              onRemove={remove}
              onRedownload={redownload}
            />
          ))}
        </AnimatePresence>
      </div>

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
    </div>
  );
}

const HistoryRow = memo(function HistoryRow({
  entry,
  onRemove,
  onRedownload,
}: {
  entry: HistoryEntry;
  onRemove: (id: number) => void;
  onRedownload: (entry: HistoryEntry) => void;
}) {
  const { t, language } = useTranslation();
  const { src } = useThumbnail(entry.thumbnailUrl);

  return (
    <motion.article
      layout="position"
      initial={{ opacity: 0, y: 6 }}
      animate={{ opacity: 1, y: 0 }}
      exit={{ opacity: 0, scale: 0.98, transition: { duration: 0.15 } }}
      transition={{ duration: 0.24, ease: [0.22, 1, 0.36, 1] }}
      className={cn(
        'group flex items-center gap-3 rounded-[var(--radius-card)] border border-[var(--border)]',
        'bg-surface p-2.5 transition-colors duration-200 hover:border-[var(--border-strong)]',
      )}
    >
      <div className="relative aspect-video w-[88px] shrink-0 overflow-hidden rounded-lg bg-surface-sunken">
        {src ? (
          <img src={src} alt="" aria-hidden="true" draggable={false} className="no-drag size-full object-cover" />
        ) : (
          <div className="flex size-full items-center justify-center">
            <PlatformBadge platform={entry.platform} size="sm" />
          </div>
        )}
      </div>

      <div className="min-w-0 flex-1">
        <h3 className="line-clamp-1 text-[13.5px] font-medium text-fg">{entry.title}</h3>
        <div className="mt-1 flex flex-wrap items-center gap-x-2 gap-y-1 text-[11.5px] text-fg-faint">
          <PlatformBadge platform={entry.platform} size="sm" />
          <Badge tone="outline">{entry.qualityLabel}</Badge>
          <Badge tone="outline">{entry.container.toUpperCase()}</Badge>
          {entry.fileSize != null && <span className="tabular">{formatBytes(entry.fileSize)}</span>}
          <span className="tabular">{formatDate(entry.createdAt, language)}</span>
        </div>
        {!entry.fileExists && (
          <p className="mt-1 flex items-center gap-1 text-[11.5px] text-warning">
            <FileWarning size={11} />
            {t('history.missing')}
          </p>
        )}
      </div>

      <div className="flex shrink-0 items-center gap-0.5">
        {entry.fileExists && (
          <>
            <IconButton
              icon={<SquareArrowOutUpRight size={14} />}
              label={t('history.open')}
              size="sm"
              onClick={() => void openPath(entry.filePath)}
            />
            <IconButton
              icon={<FolderOpen size={14} />}
              label={t('history.folder')}
              size="sm"
              onClick={() => void revealItemInDir(entry.filePath)}
            />
          </>
        )}
        <IconButton
          icon={<Download size={14} />}
          label={t('history.redownload')}
          size="sm"
          tone="accent"
          onClick={() => onRedownload(entry)}
        />
        <IconButton
          icon={<Trash2 size={14} />}
          label={t('history.delete')}
          size="sm"
          tone="danger"
          onClick={() => onRemove(entry.id)}
        />
      </div>

      <span className="sr-only">{basename(entry.filePath)}</span>
    </motion.article>
  );
});

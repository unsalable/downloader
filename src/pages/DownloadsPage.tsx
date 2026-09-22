import { AnimatePresence } from 'motion/react';
import { Download, Pause, Play } from 'lucide-react';
import { useMemo } from 'react';

import { DownloadCard } from '@/components/downloads/DownloadCard';
import { Button } from '@/components/ui/Button';
import { EmptyState } from '@/components/ui/EmptyState';
import { IconButton } from '@/components/ui/IconButton';
import { ListGroup } from '@/components/ui/ListGroup';
import { PageHeader } from '@/components/ui/PageHeader';
import { useTranslation } from '@/i18n';
import { cn } from '@/lib/cn';
import { IS_MOBILE } from '@/lib/platform';
import * as ipc from '@/services/ipc';
import { selectActive, selectFinished, useQueueStore } from '@/stores/useQueueStore';

/**
 * Defined once, outside the component. The rows are memoised, and handlers
 * recreated on every render -- which progress causes several times a second --
 * would make every row re-render on every tick instead of only the one that
 * moved.
 */
const HANDLERS = {
  onPause: (id: string) => void ipc.pauseDownload(id),
  onResume: (id: string) => void ipc.resumeDownload(id),
  onCancel: (id: string) => void ipc.cancelDownload(id),
  onRetry: (id: string) => void ipc.retryDownload(id),
  onRemove: (id: string) => void ipc.removeDownload(id),
};

/** Where a row's text starts: its padding, the 72px thumbnail, the gap. */
const TEXT_INSET = IS_MOBILE ? 96 : 104;

export function DownloadsPage({ onGoHome }: { onGoHome: () => void }) {
  const { t } = useTranslation();
  const tasks = useQueueStore((state) => state.tasks);

  // One list, newest first, whatever state a row is in. A task's creation time
  // never changes, so a progress tick rebuilds this array in the same order and
  // no row moves; the sort is stable, so the items of a gallery, queued in the
  // same instant, stay in the order they were queued.
  const { rows, anyRunning, anyPaused, anyFinished } = useMemo(
    () => ({
      rows: [...tasks].sort((a, b) => b.createdAt - a.createdAt),
      anyRunning: selectActive(tasks).length > 0,
      anyPaused: tasks.some((task) => task.status === 'paused'),
      anyFinished: selectFinished(tasks).length > 0,
    }),
    [tasks],
  );

  // Only what applies right now. Left undefined when nothing does, so a phone,
  // which shows the actions and no title, renders no header at all.
  const actions =
    anyRunning || anyPaused || anyFinished ? (
      <>
        {anyRunning && (
          <IconButton
            icon={<Pause size={15} />}
            label={t('downloads.pauseAll')}
            onClick={() => void ipc.pauseAllDownloads()}
          />
        )}
        {anyPaused && (
          <IconButton
            icon={<Play size={15} />}
            label={t('downloads.resumeAll')}
            onClick={() => void ipc.resumeAllDownloads()}
          />
        )}
        {anyFinished && (
          <Button
            size="sm"
            variant="ghost"
            aria-label={t('downloads.clearFinished')}
            onClick={() => void ipc.clearFinishedDownloads()}
          >
            {t('downloads.clear')}
          </Button>
        )}
      </>
    ) : undefined;

  return (
    <div className={cn('mx-auto w-full max-w-[760px] pb-12', IS_MOBILE ? 'px-4 pt-2' : 'px-6')}>
      <PageHeader title={t('downloads.title')} actions={actions} />

      {rows.length === 0 ? (
        <EmptyState
          icon={Download}
          title={t('downloads.emptyTitle')}
          body={t('downloads.emptyBody')}
          action={
            <Button variant="secondary" onClick={onGoHome}>
              {t('downloads.goHome')}
            </Button>
          }
        />
      ) : (
        // `relative` gives a row that is leaving something to be positioned
        // against while the rows under it close the gap.
        <ListGroup inset={TEXT_INSET} className="relative">
          <AnimatePresence initial={false} mode="popLayout">
            {rows.map((task) => (
              <DownloadCard key={task.id} task={task} {...HANDLERS} />
            ))}
          </AnimatePresence>
        </ListGroup>
      )}
    </div>
  );
}

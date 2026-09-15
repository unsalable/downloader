import { AnimatePresence, motion } from 'motion/react';
import { Eraser, Pause, Play } from 'lucide-react';
import { useMemo } from 'react';

import { DownloadCard } from '@/components/downloads/DownloadCard';
import { Button } from '@/components/ui/Button';
import { EmptyState, QueueIllustration } from '@/components/ui/EmptyState';
import { useTranslation } from '@/i18n';
import { cn } from '@/lib/cn';
import { IS_MOBILE } from '@/lib/platform';
import * as ipc from '@/services/ipc';
import {
  selectActive,
  selectFinished,
  selectQueued,
  useQueueStore,
} from '@/stores/useQueueStore';
import type { DownloadTask } from '@/types';

/**
 * Defined once, outside the component. The cards are memoised, and handlers
 * recreated on every render -- which progress causes several times a second --
 * would make every card re-render on every tick instead of only the one that
 * moved.
 */
const HANDLERS: GroupHandlers = {
  onPause: (id) => void ipc.pauseDownload(id),
  onResume: (id) => void ipc.resumeDownload(id),
  onCancel: (id) => void ipc.cancelDownload(id),
  onRetry: (id) => void ipc.retryDownload(id),
  onRemove: (id) => void ipc.removeDownload(id),
  onMove: (id, delta) => void ipc.reorderDownload(id, delta),
};

export function DownloadsPage({ onGoHome }: { onGoHome: () => void }) {
  const { t } = useTranslation();
  const tasks = useQueueStore((state) => state.tasks);

  const groups = useMemo(
    () => ({
      active: selectActive(tasks),
      queued: selectQueued(tasks),
      finished: selectFinished(tasks),
    }),
    [tasks],
  );

  const hasAnything = tasks.length > 0;
  const anyRunning = groups.active.length > 0;
  const anyPaused = groups.queued.some((task) => task.status === 'paused');

  if (!hasAnything) {
    return (
      <div className="mx-auto w-full max-w-[720px] px-6">
        <EmptyState
          illustration={<QueueIllustration />}
          title={t('downloads.emptyTitle')}
          body={t('downloads.emptyBody')}
          action={
            <Button variant="secondary" onClick={onGoHome}>
              {t('downloads.goHome')}
            </Button>
          }
        />
      </div>
    );
  }

  return (
    <div className="mx-auto w-full max-w-[760px] px-6 pb-12">
      <div
        className={cn(
          'sticky top-0 z-10 -mx-6 flex flex-wrap items-center gap-2 px-6 py-3',
          IS_MOBILE ? 'bg-bg' : 'bg-bg/85 backdrop-blur-xl',
        )}
      >
        <span className="text-[12.5px] text-fg-muted">
          {t('topbar.activeCount', { n: groups.active.length })}
        </span>
        <div className="ml-auto flex flex-wrap items-center justify-end gap-1.5">
          {anyRunning && (
            <Button
              size="sm"
              variant="ghost"
              icon={<Pause size={14} />}
              onClick={() => void ipc.pauseAllDownloads()}
            >
              {t('downloads.pauseAll')}
            </Button>
          )}
          {anyPaused && (
            <Button
              size="sm"
              variant="ghost"
              icon={<Play size={14} />}
              onClick={() => void ipc.resumeAllDownloads()}
            >
              {t('downloads.resumeAll')}
            </Button>
          )}
          {groups.finished.length > 0 && (
            <Button
              size="sm"
              variant="ghost"
              icon={<Eraser size={14} />}
              onClick={() => void ipc.clearFinishedDownloads()}
            >
              {t('downloads.clearFinished')}
            </Button>
          )}
        </div>
      </div>

      <div className="flex flex-col gap-6 pt-1">
        <Group title={t('downloads.active')} tasks={groups.active} handlers={HANDLERS} />
        <Group
          title={t('downloads.queued')}
          tasks={groups.queued}
          handlers={HANDLERS}
          reorderable
        />
        <Group title={t('downloads.finished')} tasks={groups.finished} handlers={HANDLERS} />
      </div>
    </div>
  );
}

interface GroupHandlers {
  onPause: (id: string) => void;
  onResume: (id: string) => void;
  onCancel: (id: string) => void;
  onRetry: (id: string) => void;
  onRemove: (id: string) => void;
  onMove: (id: string, delta: number) => void;
}

function Group({
  title,
  tasks,
  handlers,
  reorderable = false,
}: {
  title: string;
  tasks: DownloadTask[];
  handlers: GroupHandlers;
  reorderable?: boolean;
}) {
  if (tasks.length === 0) return null;

  return (
    // Layout animation measures the section on every render, and progress
    // renders it several times a second; a phone skips the glide.
    <motion.section layout={IS_MOBILE ? false : 'position'}>
      <h2 className="mb-2 px-1 text-[11px] font-semibold uppercase tracking-[0.08em] text-fg-faint">
        {title}
        <span className="tabular ml-1.5 font-normal text-fg-faint/70">{tasks.length}</span>
      </h2>
      <div className="flex flex-col gap-2">
        <AnimatePresence initial={false} mode="popLayout">
          {tasks.map((task) => (
            <DownloadCard key={task.id} task={task} reorderable={reorderable} {...handlers} />
          ))}
        </AnimatePresence>
      </div>
    </motion.section>
  );
}

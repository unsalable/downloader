import { AnimatePresence, motion, useReducedMotionConfig } from 'motion/react';
import { Download, Pause, Play } from 'lucide-react';
import { useEffect, useMemo, useState } from 'react';
import { create } from 'zustand';

import { DownloadCard } from '@/components/downloads/DownloadCard';
import { HistoryList } from '@/components/history/HistoryList';
import { Button } from '@/components/ui/Button';
import { EmptyState } from '@/components/ui/EmptyState';
import { IconButton } from '@/components/ui/IconButton';
import { ListGroup } from '@/components/ui/ListGroup';
import { PageHeader } from '@/components/ui/PageHeader';
import { Segmented } from '@/components/ui/Segmented';
import { useTranslation } from '@/i18n';
import { SIDEWAYS } from '@/lib/motion';
import { IS_MOBILE } from '@/lib/platform';
import * as ipc from '@/services/ipc';
import {
  selectActive,
  selectFinished,
  selectInFlightCount,
  useQueueStore,
} from '@/stores/useQueueStore';
import type { DownloadTask } from '@/types';

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

/** How a removed row leaves; see the same choice in `HistoryList`. */
const LEAVING = IS_MOBILE ? 'sync' : 'popLayout';

export function DownloadsPage({ onGoHome }: { onGoHome: () => void }) {
  if (IS_MOBILE) return <PhoneDownloads onGoHome={onGoHome} />;

  return (
    <div className="mx-auto w-full max-w-[760px] px-6 pb-12">
      <CurrentDownloads onGoHome={onGoHome} />
    </div>
  );
}

// -- the phone's two halves ---------------------------------------------------

export type DownloadsSegment = 'active' | 'history';

/**
 * Which half of Downloads the phone shows, once it has shown one. Kept outside
 * the screen, which is unmounted whenever another tab is
 * open, so coming back finds the half that was left -- and in a store rather
 * than a plain variable, so a request for History that arrives while Downloads
 * is already on screen switches it there.
 */
const useSegmentStore = create<{ chosen: DownloadsSegment | null }>(() => ({ chosen: null }));

/**
 * Settle which half Downloads opens at on the phone: the user's own pick, or a
 * request the app makes for them -- History asked for by name, or the list of
 * current downloads just after one was started.
 */
export function showDownloadsSegment(segment: DownloadsSegment) {
  useSegmentStore.setState({ chosen: segment });
}

/** Anything still to finish, including what is waiting or paused. */
function hasWork(tasks: DownloadTask[]): boolean {
  return selectInFlightCount(tasks) > 0 || tasks.some((task) => task.status === 'paused');
}

/**
 * A phone has no room for a History tab, so History is the second half of
 * this screen, behind a two-way switch at its top.
 */
function PhoneDownloads({ onGoHome }: { onGoHome: () => void }) {
  const { t } = useTranslation();
  const chosen = useSegmentStore((state) => state.chosen);

  // The first time, the screen opens where something is to be seen: the
  // running downloads if there are any, the finished ones if not. Read once
  // rather than subscribed to, so a download finishing while the screen is open
  // does not pull the list away from under the user -- and kept once shown, so
  // it does not either between one visit and the next.
  const [fallback] = useState<DownloadsSegment>(() =>
    hasWork(useQueueStore.getState().tasks) ? 'active' : 'history',
  );
  const segment = chosen ?? fallback;
  useEffect(() => {
    if (chosen == null) showDownloadsSegment(fallback);
  }, [chosen, fallback]);

  // The new list comes in from the side of the half that was picked, and
  // under the system's reduced motion only fades: a transform string is beyond
  // the reach of `MotionConfig` (see App).
  const still = useReducedMotionConfig() ?? false;
  const direction = still ? 0 : segment === 'history' ? 1 : -1;

  return (
    <div className="mx-auto w-full max-w-[760px] px-4 pb-12 pt-2">
      <Segmented
        value={segment}
        onChange={showDownloadsSegment}
        options={[
          { value: 'active', label: t('downloads.active') },
          { value: 'history', label: t('nav.history') },
        ]}
      />

      <AnimatePresence mode="wait" initial={false} custom={direction}>
        <motion.div
          key={segment}
          custom={direction}
          variants={SIDEWAYS}
          initial="initial"
          animate="animate"
          exit="exit"
          className="mt-3"
        >
          {segment === 'active' ? (
            <CurrentDownloads onGoHome={onGoHome} />
          ) : (
            <HistoryList onGoHome={onGoHome} />
          )}
        </motion.div>
      </AnimatePresence>
    </div>
  );
}

// -- the downloads themselves ---------------------------------------------------

/**
 * What is downloading, waiting, or finished and not yet cleared away, with the
 * actions that apply to all of it. On the desktop this is the whole screen; on
 * a phone, its first half.
 */
function CurrentDownloads({ onGoHome }: { onGoHome: () => void }) {
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
    <>
      <PageHeader
        title={t('downloads.title')}
        actions={actions}
        // Under the switch, the actions start the half, so they take no room
        // above them.
        className={IS_MOBILE ? 'pt-0' : undefined}
      />

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
          <AnimatePresence initial={false} mode={LEAVING}>
            {rows.map((task) => (
              <DownloadCard key={task.id} task={task} {...HANDLERS} />
            ))}
          </AnimatePresence>
        </ListGroup>
      )}
    </>
  );
}

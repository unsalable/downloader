import { useEffect, useState } from 'react';

import { ListGroup, ListGroupLabel, ROW_LINE } from '@/components/ui/ListGroup';
import { PlatformBadge } from '@/components/ui/PlatformBadge';
import { Spinner } from '@/components/ui/Spinner';
import { useThumbnail } from '@/hooks/useThumbnail';
import { useTranslation } from '@/i18n';
import { cn } from '@/lib/cn';
import { VIDEO_EXTENSIONS } from '@/lib/editor/files';
import { formatBytes, formatDate, formatDay } from '@/lib/format';
import { IS_MOBILE } from '@/lib/platform';
import * as ipc from '@/services/ipc';
import { useQueueStore } from '@/stores/useQueueStore';
import type { HistoryEntry } from '@/types';

/** How many rows are offered. */
const SHOWN = 6;

/**
 * How far back the history is read to find them. Sound, pictures and files
 * that have since gone are passed over, so more is read than is shown -- and
 * an album or a photo post can leave dozens of rows at once.
 */
const SCANNED = 200;

/** Where a row's text starts: its padding, the 72px thumbnail, the gap. */
const TEXT_INSET = IS_MOBILE ? 96 : 104;

const EDITABLE = new Set(VIDEO_EXTENSIONS);

/**
 * Only a finished video that is still where it was saved. The editor turns a
 * file with no picture away, and a row that leads to that refusal is not worth
 * showing.
 *
 * The container decides, with one exception: sound kept in its source's own
 * container can be a WebM with no picture in it. A request for pictures is no
 * such tell -- a carousel that opens on a photo asks for pictures for all of
 * its items, and its videos still arrive as MP4.
 */
function isEditable(entry: HistoryEntry): boolean {
  return (
    entry.status === 'completed' &&
    entry.fileExists &&
    entry.request?.mode !== 'audio' &&
    EDITABLE.has(entry.container.toLowerCase())
  );
}

/**
 * The newest of each file. Downloading a link again after its file was
 * deleted writes to the same path and adds a second record of it.
 */
function newestEach(entries: HistoryEntry[]): HistoryEntry[] {
  const seen = new Set<string>();
  return entries.filter((entry) => {
    if (seen.has(entry.filePath)) return false;
    seen.add(entry.filePath);
    return true;
  });
}

/**
 * The last videos downloaded, under the empty editor. A video that has just
 * landed is most often the one about to be cut, and pressing it here is quicker
 * than finding it again through the file picker. When there are none, nothing
 * is shown: the card above already says what to do.
 */
export function RecentDownloads({
  onOpen,
  disabled = false,
  className,
}: {
  /** Resolves to null when the file would not open. */
  onOpen: (path: string) => Promise<unknown>;
  disabled?: boolean;
  className?: string;
}) {
  const { t } = useTranslation();
  const [entries, setEntries] = useState<HistoryEntry[]>([]);
  const [pending, setPending] = useState<string | null>(null);
  // Bumped when a row would not open, most likely because its file has gone
  // since the list was read; reading it again takes that row away.
  const [stale, setStale] = useState(0);

  // Read again whenever another download finishes, so one that lands while the
  // screen is open is offered without leaving it. A count rather than the
  // list, which changes several times a second while anything is downloading.
  const finished = useQueueStore(
    (state) => state.tasks.filter((task) => task.status === 'completed').length,
  );

  useEffect(() => {
    let live = true;
    ipc
      .listHistory(undefined, SCANNED, 0)
      .then((rows) => {
        if (live) setEntries(newestEach(rows.filter(isEditable)).slice(0, SHOWN));
      })
      // Nothing to offer is what an unreadable history looks like here, too.
      .catch(() => undefined);
    return () => {
      live = false;
    };
  }, [finished, stale]);

  if (entries.length === 0) return null;

  const open = async (path: string) => {
    setPending(path);
    try {
      if ((await onOpen(path)) == null) setStale((count) => count + 1);
    } finally {
      setPending(null);
    }
  };

  return (
    <section className={className}>
      <ListGroupLabel className="mb-2">{t('editor.recent')}</ListGroupLabel>
      <ListGroup inset={TEXT_INSET}>
        {entries.map((entry) => (
          <RecentRow
            key={entry.id}
            entry={entry}
            pending={pending === entry.filePath}
            disabled={disabled || pending != null}
            onOpen={open}
          />
        ))}
      </ListGroup>
    </section>
  );
}

function RecentRow({
  entry,
  pending,
  disabled,
  onOpen,
}: {
  entry: HistoryEntry;
  pending: boolean;
  disabled: boolean;
  onOpen: (path: string) => void;
}) {
  const { t, language } = useTranslation();
  const { src } = useThumbnail(entry.thumbnailUrl);

  return (
    <button
      type="button"
      disabled={disabled}
      onClick={() => onOpen(entry.filePath)}
      aria-label={t('editor.openRecent', { title: entry.title })}
      aria-busy={pending || undefined}
      className={cn(
        'flex w-full items-center py-2.5 text-left',
        IS_MOBILE ? 'gap-3 pl-3 pr-4' : 'gap-4 pl-4 pr-4',
        'transition-colors duration-150 ease-out-quint hover:bg-surface-hover active:bg-surface-active',
        // Every row stands still while one opens; only the one pressed says so.
        'disabled:pointer-events-none',
      )}
    >
      <span className="flex h-11 w-[72px] shrink-0 items-center justify-center overflow-hidden rounded-[var(--radius-thumb)] bg-surface-sunken">
        {src ? (
          <img
            src={src}
            alt=""
            aria-hidden="true"
            draggable={false}
            className="no-drag size-full object-cover"
          />
        ) : (
          <PlatformBadge platform={entry.platform} size="md" />
        )}
      </span>

      <span className="block min-w-0 flex-1">
        <span className="block truncate text-[13.5px] font-medium leading-[18px] text-fg">
          {entry.title}
        </span>
        <span className={cn(ROW_LINE, 'mt-0.5 gap-x-3')}>
          {/* A phone has room for the day, not for the year and the minute. */}
          <span>
            {IS_MOBILE ? formatDay(entry.createdAt, language) : formatDate(entry.createdAt, language)}
          </span>
          <span>{entry.qualityLabel}</span>
          {entry.fileSize != null && <span>{formatBytes(entry.fileSize)}</span>}
        </span>
      </span>

      {pending && <Spinner size={16} className="text-fg-muted" />}
    </button>
  );
}

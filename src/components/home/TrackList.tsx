import { Check } from 'lucide-react';
import type { Dispatch, SetStateAction } from 'react';

import { ListGroup } from '@/components/ui/ListGroup';
import { useTranslation } from '@/i18n';
import { cn } from '@/lib/cn';
import { formatDuration } from '@/lib/format';
import { IS_MOBILE } from '@/lib/platform';
import type { TrackSummary } from '@/types';

interface TrackListProps {
  tracks: TrackSummary[];
  /** Positions of the songs that will be downloaded. */
  picked: ReadonlySet<number>;
  onChange: Dispatch<SetStateAction<Set<number>>>;
}

/**
 * The songs of an album or playlist, each one picked or not. Every row is its
 * own button; the list scrolls inside its card so a hundred songs do not push
 * the download button off the screen.
 */
export function TrackList({ tracks, picked, onChange }: TrackListProps) {
  const { t } = useTranslation();
  const all = picked.size === tracks.length;

  // From the latest choice rather than the one this render saw, so two quick
  // presses both count.
  const toggle = (position: number) =>
    onChange((current) => {
      const next = new Set(current);
      if (next.has(position)) next.delete(position);
      else next.add(position);
      return next;
    });

  return (
    <section>
      <div className="mb-2 flex items-center justify-between px-4">
        <h2 className="text-[13px] font-semibold text-fg-muted">{t('tracks.title')}</h2>
        <button
          type="button"
          onClick={() => onChange(all ? new Set() : new Set(tracks.map((track) => track.position)))}
          className="pressable -mr-1.5 rounded-md px-1.5 py-0.5 text-[12.5px] font-medium text-accent"
        >
          {all ? t('tracks.selectNone') : t('tracks.selectAll')}
        </button>
      </div>

      <ListGroup inset={IS_MOBILE ? 56 : 52} className="max-h-[360px] overflow-y-auto">
        {tracks.map((track) => {
          const on = picked.has(track.position);
          return (
            <button
              key={track.position}
              type="button"
              role="checkbox"
              aria-checked={on}
              onClick={() => toggle(track.position)}
              className={cn(
                'flex w-full items-center gap-3 px-4 text-left hover:bg-surface-hover',
                IS_MOBILE ? 'min-h-[56px] py-2' : 'min-h-[48px] py-1.5',
              )}
            >
              <span
                aria-hidden="true"
                className={cn(
                  'flex size-[22px] shrink-0 items-center justify-center rounded-full transition-colors duration-150',
                  on ? 'bg-accent text-white' : 'border-[1.5px] border-fg-faint',
                )}
              >
                {on && <Check size={13} strokeWidth={3} />}
              </span>

              <span className="min-w-0 flex-1">
                <span
                  className={cn(
                    'block truncate text-fg',
                    IS_MOBILE ? 'text-[15px]' : 'text-[13.5px]',
                    !on && 'text-fg-muted',
                  )}
                >
                  {track.title}
                </span>
                {track.artists && (
                  <span className="block truncate text-[12.5px] text-fg-muted">{track.artists}</span>
                )}
              </span>

              {track.durationSec != null && (
                <span className="tabular shrink-0 text-[12.5px] text-fg-faint">
                  {formatDuration(track.durationSec)}
                </span>
              )}
            </button>
          );
        })}
      </ListGroup>
    </section>
  );
}

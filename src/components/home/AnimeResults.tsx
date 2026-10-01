import { motion } from 'motion/react';

import { ListGroup, ROW_LINE } from '@/components/ui/ListGroup';
import { InlineNotice } from '@/components/ui/InlineNotice';
import { Spinner } from '@/components/ui/Spinner';
import { useThumbnail } from '@/hooks/useThumbnail';
import { useTranslation } from '@/i18n';
import { cn } from '@/lib/cn';
import { formatDuration } from '@/lib/format';
import { RISE } from '@/lib/motion';
import { IS_MOBILE } from '@/lib/platform';
import type { AnimeEpisode } from '@/types';

export type AnimeSearch =
  | { phase: 'searching'; query: string }
  | { phase: 'done'; query: string; episodes: AnimeEpisode[] }
  | { phase: 'error'; query: string; message: string };

interface AnimeResultsProps {
  search: AnimeSearch;
  onPick: (episode: AnimeEpisode) => void;
}

/**
 * Whole episodes from the channels that license them. Picking one reads it
 * like any pasted link.
 */
export function AnimeResults({ search, onPick }: AnimeResultsProps) {
  const { t } = useTranslation();

  return (
    <motion.section variants={RISE} initial="initial" animate="animate" exit="exit">
      <h2 className="mb-2 px-4 text-[13px] font-semibold text-fg-muted">{t('anime.title')}</h2>

      {search.phase === 'searching' && (
        <div className="flex items-center justify-center gap-2 py-8 text-[13px] text-fg-muted">
          <Spinner size={15} />
          {t('anime.searching')}
        </div>
      )}

      {search.phase === 'error' && <InlineNotice tone="error">{search.message}</InlineNotice>}

      {search.phase === 'done' && search.episodes.length === 0 && (
        <p className="px-4 py-6 text-center text-[13px] leading-relaxed text-fg-muted">
          {t('anime.empty')}
        </p>
      )}

      {search.phase === 'done' && search.episodes.length > 0 && (
        <ListGroup inset={104}>
          {search.episodes.map((episode) => (
            <EpisodeRow key={episode.url} episode={episode} onPick={onPick} />
          ))}
        </ListGroup>
      )}
    </motion.section>
  );
}

function EpisodeRow({
  episode,
  onPick,
}: {
  episode: AnimeEpisode;
  onPick: (episode: AnimeEpisode) => void;
}) {
  const { src } = useThumbnail(episode.thumbnailUrl);

  return (
    <button
      type="button"
      onClick={() => onPick(episode)}
      className={cn(
        'flex w-full items-center gap-4 px-4 text-left hover:bg-surface-hover',
        IS_MOBILE ? 'py-3' : 'py-2.5',
      )}
    >
      <div className="flex h-11 w-[72px] shrink-0 overflow-hidden rounded-[var(--radius-thumb)] bg-surface-sunken">
        {src && (
          <img src={src} alt="" aria-hidden="true" draggable={false} className="no-drag size-full object-cover" />
        )}
      </div>
      <div className="min-w-0 flex-1">
        <span className="line-clamp-2 text-[13.5px] font-medium leading-[18px] text-fg">
          {episode.title}
        </span>
        <span className={cn(ROW_LINE, 'mt-0.5 gap-x-2')}>
          <span>{episode.channel}</span>
          {episode.durationSec != null && <span>{formatDuration(episode.durationSec)}</span>}
        </span>
      </div>
    </button>
  );
}

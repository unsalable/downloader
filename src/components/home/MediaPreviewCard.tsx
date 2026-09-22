import { Eye, Heart, ImageOff, Images, Music, Radio } from 'lucide-react';

import { InlineNotice } from '@/components/ui/InlineNotice';
import { PlatformBadge } from '@/components/ui/PlatformBadge';
import { useThumbnail } from '@/hooks/useThumbnail';
import { useTranslation } from '@/i18n';
import { cn } from '@/lib/cn';
import { formatCount, formatDuration, formatUploadDate } from '@/lib/format';
import type { MediaMetadata } from '@/types';

/** A fact laid over the thumbnail: the running time, "live", the item count. */
const CHIP =
  'absolute flex items-center gap-1 rounded-md bg-black/70 px-1.5 py-0.5 text-[12px] font-medium text-white';

export function MediaPreviewCard({ metadata }: { metadata: MediaMetadata }) {
  const { t, language } = useTranslation();
  const { src, loading, failed } = useThumbnail(metadata.thumbnailUrl);

  const uploaded = formatUploadDate(metadata.uploadDate, language);
  const isAudio = metadata.mediaKind === 'audio';
  // A photo is shown whole: cropping a portrait picture to fill a 16:9 frame
  // would preview something other than what gets downloaded.
  const isPhoto =
    metadata.formats.some((format) => format.kind === 'image') &&
    !metadata.formats.some((format) => format.hasVideo);

  return (
    <article className="overflow-hidden rounded-[var(--radius-card)] border border-card-edge bg-surface">
      <div className="relative aspect-video w-full overflow-hidden bg-surface-sunken">
        {loading && <div aria-hidden="true" className="skeleton absolute inset-0" />}

        {src && (
          <img
            src={src}
            alt=""
            draggable={false}
            className={cn('no-drag size-full', isPhoto ? 'object-contain' : 'object-cover')}
            // Decorative: the title beneath carries the meaning.
            aria-hidden="true"
          />
        )}

        {(failed || (!src && !loading)) && (
          <div className="flex size-full flex-col items-center justify-center gap-2 text-fg-faint">
            {isAudio ? <Music size={26} /> : <ImageOff size={24} />}
          </div>
        )}

        {/* The chips carry their own dark backing, so they read over a bright
            frame without a scrim dimming the picture. */}
        {metadata.isLive ? (
          <span className={cn(CHIP, 'bottom-2.5 right-2.5')}>
            <Radio size={12} />
            {t('preview.live')}
          </span>
        ) : (
          metadata.durationSec != null && (
            <span className={cn(CHIP, 'tabular bottom-2.5 right-2.5')}>
              {formatDuration(metadata.durationSec)}
            </span>
          )
        )}

        {metadata.entryCount != null && metadata.entryCount > 1 && (
          <span className={cn(CHIP, 'left-2.5 top-2.5')}>
            <Images size={12} />
            {t('preview.entries', { n: metadata.entryCount })}
          </span>
        )}
      </div>

      <div className="p-4">
        <h3 className="line-clamp-2 text-[15px] font-semibold leading-snug tracking-[-0.01em] text-fg">
          {metadata.title}
        </h3>

        {/* Set apart by the gap alone; the icons already mark where one figure
            ends and the next begins. */}
        <div className="mt-2 flex flex-wrap items-center gap-x-3 gap-y-1 text-[12.5px] text-fg-muted">
          <span className="flex items-center gap-1.5">
            {/* The name is right beside it, so the tile need not say it again. */}
            <span aria-hidden="true" className="flex">
              <PlatformBadge platform={metadata.platform} size="sm" />
            </span>
            {metadata.platformLabel}
          </span>

          {metadata.creator && <span className="truncate">{metadata.creator}</span>}

          {metadata.viewCount != null && (
            <span className="tabular flex items-center gap-1">
              <Eye size={13} />
              {formatCount(metadata.viewCount)}
            </span>
          )}

          {metadata.likeCount != null && (
            <span className="tabular flex items-center gap-1">
              <Heart size={13} />
              {formatCount(metadata.likeCount)}
            </span>
          )}

          {uploaded && <span>{uploaded}</span>}
        </div>

        {/* Notes the source itself could not tell us -- currently only that no
            provider recognised the site, so the result is a best-effort read of
            the page's own tags. */}
        {metadata.warnings.includes('generic') && (
          <InlineNotice className="mt-3">{t('preview.warningGeneric')}</InlineNotice>
        )}
      </div>
    </article>
  );
}

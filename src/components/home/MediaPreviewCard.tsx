import { motion } from 'motion/react';
import { Eye, Heart, ImageOff, Images, Info, Music, Radio } from 'lucide-react';

import { Badge } from '@/components/ui/Badge';
import { PlatformBadge } from '@/components/ui/PlatformBadge';
import { Skeleton } from '@/components/ui/Skeleton';
import { useThumbnail } from '@/hooks/useThumbnail';
import { useTranslation } from '@/i18n';
import { cn } from '@/lib/cn';
import { formatCount, formatDuration, formatUploadDate } from '@/lib/format';
import { T } from '@/lib/motion';
import type { MediaMetadata } from '@/types';

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
    <motion.article
      initial={{ opacity: 0, y: 12, scale: 0.985 }}
      animate={{ opacity: 1, y: 0, scale: 1 }}
      transition={T.spatial}
      className={cn(
        'overflow-hidden rounded-[var(--radius-panel)] border border-[var(--border)]',
        'bg-surface shadow-raised edge-light',
      )}
    >
      <div className="relative aspect-video w-full overflow-hidden bg-surface-sunken">
        {loading && <Skeleton className="absolute inset-0" rounded="sm" />}

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

        {/* Scrim so the overlaid chips stay readable over a bright frame. */}
        <div className="pointer-events-none absolute inset-x-0 bottom-0 h-20 bg-[linear-gradient(to_top,rgb(0_0_0/0.62),transparent)]" />

        <div className="absolute bottom-2.5 right-2.5 flex items-center gap-1.5">
          {metadata.isLive ? (
            <span className="flex items-center gap-1 rounded-md bg-[var(--error)] px-1.5 py-0.5 text-[10.5px] font-semibold uppercase tracking-wide text-white">
              <Radio size={10} />
              {t('preview.live')}
            </span>
          ) : (
            metadata.durationSec != null && (
              <span className="tabular rounded-md bg-black/70 px-1.5 py-0.5 text-[11px] font-medium text-white">
                {formatDuration(metadata.durationSec)}
              </span>
            )
          )}
        </div>

        {metadata.entryCount != null && metadata.entryCount > 1 && (
          <span className="absolute left-2.5 top-2.5 flex items-center gap-1 rounded-md bg-black/70 px-1.5 py-0.5 text-[11px] font-medium text-white">
            <Images size={11} />
            {t('preview.entries', { n: metadata.entryCount })}
          </span>
        )}
      </div>

      <div className="p-4">
        <h3 className="line-clamp-2 text-[15px] font-semibold leading-snug tracking-[-0.01em] text-fg">
          {metadata.title}
        </h3>

        <div className="mt-2 flex flex-wrap items-center gap-x-2.5 gap-y-1 text-[12.5px] text-fg-muted">
          <span className="flex items-center gap-1.5">
            <PlatformBadge platform={metadata.platform} size="sm" />
            {metadata.platformLabel}
          </span>

          {metadata.creator && (
            <>
              <Dot />
              <span className="truncate">{metadata.creator}</span>
            </>
          )}

          {metadata.viewCount != null && (
            <>
              <Dot />
              <span className="tabular flex items-center gap-1">
                <Eye size={12} />
                {formatCount(metadata.viewCount)}
              </span>
            </>
          )}

          {metadata.likeCount != null && (
            <>
              <Dot />
              <span className="tabular flex items-center gap-1">
                <Heart size={12} />
                {formatCount(metadata.likeCount)}
              </span>
            </>
          )}

          {uploaded && (
            <>
              <Dot />
              <span>{uploaded}</span>
            </>
          )}
        </div>

        <QualityChips metadata={metadata} />

        {/* Notes the source itself could not tell us -- currently only that no
            provider recognised the site, so the result is a best-effort read of
            the page's own tags. */}
        {metadata.warnings.includes('generic') && (
          <div className="mt-3 flex items-start gap-2 rounded-lg bg-[var(--surface-sunken)] px-3 py-2">
            <Info size={13} className="mt-0.5 shrink-0 text-fg-faint" />
            <p className="text-[12px] leading-relaxed text-fg-muted">
              {t('preview.warningGeneric')}
            </p>
          </div>
        )}
      </div>
    </motion.article>
  );
}

function Dot() {
  return <span className="text-fg-faint">·</span>;
}

/** The top few renditions, as a quick "what am I getting" glance. */
function QualityChips({ metadata }: { metadata: MediaMetadata }) {
  const seen = new Set<string>();
  const chips: { label: string; tone: 'accent' | 'neutral' }[] = [];

  const best = metadata.formats.find((format) => format.hasVideo) ?? metadata.formats[0];
  if (best) {
    chips.push({ label: best.qualityLabel, tone: 'accent' });
    seen.add(best.qualityLabel);
  }

  for (const format of metadata.formats) {
    if (chips.length >= 5) break;
    if (seen.has(format.qualityLabel)) continue;
    seen.add(format.qualityLabel);
    chips.push({ label: format.qualityLabel, tone: 'neutral' });
  }

  if (chips.length === 0) return null;

  return (
    <div className="mt-3 flex flex-wrap items-center gap-1.5">
      {chips.map((chip) => (
        <Badge key={chip.label} tone={chip.tone}>
          {chip.label}
        </Badge>
      ))}
    </div>
  );
}

import { motion } from 'motion/react';

import { Skeleton } from '@/components/ui/Skeleton';
import { useTranslation } from '@/i18n';
import { cn } from '@/lib/cn';

/**
 * Skeleton shown while the engine resolves a link. It mirrors the real preview
 * card's proportions, so the transition to actual content is a fill rather than
 * a layout jump.
 */
export function AnalyzingCard() {
  const { t } = useTranslation();

  return (
    <motion.div
      initial={{ opacity: 0, y: 10 }}
      animate={{ opacity: 1, y: 0 }}
      exit={{ opacity: 0, y: -8 }}
      transition={{ duration: 0.28, ease: [0.22, 1, 0.36, 1] }}
      className={cn(
        'overflow-hidden rounded-[var(--radius-panel)] border border-[var(--border)]',
        'bg-surface shadow-raised edge-light',
      )}
      role="status"
      aria-live="polite"
      aria-label={t('analyze.analyzing')}
    >
      <Skeleton className="aspect-video w-full" rounded="sm" />

      <div className="p-4">
        <div className="flex items-center gap-2 text-[13px] font-medium text-fg-muted">
          <span className="size-1.5 shrink-0 bg-accent" />
          <span className="eyebrow font-mono">{t('analyze.analyzing')}</span>
        </div>

        <Skeleton className="mt-3 h-4 w-4/5" />
        <Skeleton className="mt-2 h-4 w-2/5" />

        <div className="mt-4 flex gap-1.5">
          <Skeleton className="h-5 w-14" />
          <Skeleton className="h-5 w-12" />
          <Skeleton className="h-5 w-16" />
        </div>
      </div>

      <div className="border-t border-[var(--border)] p-4">
        <div className="grid grid-cols-2 gap-3">
          <Skeleton className="h-10" />
          <Skeleton className="h-10" />
          <Skeleton className="h-10" />
          <Skeleton className="h-10" />
        </div>
        <Skeleton className="mt-4 h-12 w-full" rounded="lg" />
      </div>
    </motion.div>
  );
}

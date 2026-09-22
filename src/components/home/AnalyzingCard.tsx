import { motion } from 'motion/react';

import { Skeleton } from '@/components/ui/Skeleton';
import { useTranslation } from '@/i18n';
import { cn } from '@/lib/cn';
import { rise } from '@/lib/motion';
import { IS_MOBILE } from '@/lib/platform';

const CARD = 'overflow-hidden rounded-[var(--radius-card)] border border-card-edge bg-surface';

/**
 * Skeleton shown while the engine resolves a link. It mirrors the preview card
 * and the options panel that replace it, so the change to real content is a
 * fill rather than a layout jump. The spinner in the field says what is going
 * on; this only holds the space, and tells a screen reader the same thing.
 */
export function AnalyzingCard() {
  const { t } = useTranslation();
  const field = IS_MOBILE ? 'h-12' : 'h-9';

  return (
    <motion.div
      variants={rise(10)}
      initial="initial"
      animate="animate"
      exit="exit"
      className="flex flex-col gap-4"
      role="status"
      aria-live="polite"
      aria-label={t('analyze.analyzing')}
    >
      <div className={CARD}>
        {/* Square-cornered: the card clips it, as it does the real thumbnail. */}
        <div aria-hidden="true" className="skeleton aspect-video w-full" />
        <div className="p-4">
          <Skeleton className="h-4 w-4/5" />
          <Skeleton className="mt-3 h-3.5 w-2/5" />
        </div>
      </div>

      <div className={cn(CARD, 'p-4')}>
        <div className="grid grid-cols-2 gap-3">
          <Skeleton className={field} />
          <Skeleton className={field} />
        </div>
        <Skeleton className={cn('mt-3', field)} />
        <Skeleton className={cn('mt-4', IS_MOBILE ? 'h-13' : 'h-11')} rounded="lg" />
      </div>
    </motion.div>
  );
}

import { AnimatePresence, motion } from 'motion/react';

import { PlatformBadge } from '@/components/ui/PlatformBadge';
import { useTranslation } from '@/i18n';
import { cn } from '@/lib/cn';
import { FADE, T } from '@/lib/motion';
import { FEATURED_PLATFORMS, platformPresentation } from '@/lib/platforms';
import type { PlatformId } from '@/types';

/**
 * Shows what the pasted link was recognised as. Detection happens in Rust so
 * there is only one copy of the host patterns; this just renders the answer.
 */
export function PlatformIndicator({ platform }: { platform: PlatformId }) {
  const { t } = useTranslation();
  const known = platform !== 'unknown' && platform !== 'generic';
  const presentation = platformPresentation(platform);

  return (
    <div className="flex h-8 items-center justify-center">
      <AnimatePresence mode="wait">
        {known ? (
          <motion.div
            key={platform}
            initial={{ opacity: 0, y: 6, scale: 0.96 }}
            animate={{ opacity: 1, y: 0, scale: 1 }}
            exit={{ opacity: 0, y: -6, scale: 0.96, transition: T.componentOut }}
            transition={T.component}
            className={cn(
              'flex items-center gap-2 rounded-full border border-[var(--border)]',
              'bg-surface py-1 pl-1 pr-3 shadow-soft',
            )}
          >
            <PlatformBadge platform={platform} size="sm" />
            <span className="text-[12px] text-fg-faint">{t('input.detected')}</span>
            <span className="text-[12.5px] font-medium text-fg">{presentation.label}</span>
          </motion.div>
        ) : (
          <motion.div
            key="featured"
            variants={FADE}
            initial="initial"
            animate="animate"
            exit="exit"
            className="flex items-center gap-1.5"
          >
            {FEATURED_PLATFORMS.map((id) => (
              <PlatformBadge
                key={id}
                platform={id}
                size="sm"
                className="opacity-70 transition-opacity duration-150 ease-out-quint hover:opacity-100"
              />
            ))}
          </motion.div>
        )}
      </AnimatePresence>
    </div>
  );
}

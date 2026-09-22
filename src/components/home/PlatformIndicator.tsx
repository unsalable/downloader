import { AnimatePresence, motion } from 'motion/react';

import { PlatformBadge } from '@/components/ui/PlatformBadge';
import { FADE } from '@/lib/motion';
import { FEATURED_PLATFORMS, platformPresentation } from '@/lib/platforms';
import type { PlatformId } from '@/types';

/** The featured row at rest: 28px tiles, 10px apart. */
const FEATURED_WIDTH = FEATURED_PLATFORMS.length * 28 + (FEATURED_PLATFORMS.length - 1) * 10;

/**
 * Shows what the pasted link was recognised as, and before there is a link, a
 * row of the sites people most often come here for. Detection happens in Rust
 * so there is only one copy of the host patterns; this just renders the answer.
 */
export function PlatformIndicator({ platform }: { platform: PlatformId }) {
  const known = platform !== 'unknown' && platform !== 'generic';

  return (
    <div className="flex h-8 items-center justify-center">
      <AnimatePresence mode="wait" initial={false}>
        {known ? (
          <motion.div
            key={platform}
            variants={FADE}
            initial="initial"
            animate="animate"
            exit="exit"
            className="flex items-center gap-2 rounded-full bg-fill py-1 pl-1 pr-3"
          >
            {/* The name is right beside it, so the tile need not say it again. */}
            <span aria-hidden="true" className="flex">
              <PlatformBadge platform={platform} size="sm" />
            </span>
            <span className="text-[12.5px] font-medium text-fg">
              {platformPresentation(platform).label}
            </span>
          </motion.div>
        ) : (
          // Spread across a fixed width rather than set a fixed gap apart: the
          // gap is 10px wherever the row fits, and closes up by a pixel or two
          // on a narrow phone instead of pushing the last tile off the screen.
          <motion.div
            key="featured"
            variants={FADE}
            initial="initial"
            animate="animate"
            exit="exit"
            className="flex w-full items-center justify-between"
            style={{ maxWidth: FEATURED_WIDTH }}
          >
            {FEATURED_PLATFORMS.map((id) => (
              <PlatformBadge key={id} platform={id} size="md" />
            ))}
          </motion.div>
        )}
      </AnimatePresence>
    </div>
  );
}

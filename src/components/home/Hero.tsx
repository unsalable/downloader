import { motion } from 'motion/react';

import { useTranslation } from '@/i18n';
import { cn } from '@/lib/cn';
import { T } from '@/lib/motion';
import { IS_MOBILE } from '@/lib/platform';

/**
 * The Home headline. Entrance is a short, staggered rise -- it plays once on
 * mount and is then inert, so nothing keeps animating behind the input.
 */
export function Hero() {
  const { t } = useTranslation();

  return (
    <div className="text-center">
      <motion.h2
        initial={{ opacity: 0, y: 14 }}
        animate={{ opacity: 1, y: 0 }}
        transition={T.entrance}
        className={cn(
          'font-semibold leading-[1.04] tracking-[-0.045em] text-fg',
          IS_MOBILE ? 'text-[36px]' : 'text-[44px]',
        )}
      >
        {t('hero.title1')}
        <br />
        <span className="text-accent">{t('hero.title2')}</span>
      </motion.h2>

      {/* The promise reads as a specification line rather than a tagline: mono,
          spaced, ruled off on both sides.

          The rules only work while the line is one line. A phone is too narrow
          for it, and a rule hanging beside a two-line block points at nothing,
          so there the text stands on its own. */}
      <motion.div
        initial={{ opacity: 0 }}
        animate={{ opacity: 1 }}
        transition={{ ...T.entrance, delay: 0.12 }}
        className={cn(
          'mt-5 flex items-center justify-center',
          IS_MOBILE ? 'px-6' : 'gap-3',
        )}
      >
        {!IS_MOBILE && <span className="h-px w-6 bg-[var(--border-strong)]" />}
        <p className="eyebrow font-mono text-fg-faint">{t('hero.subtitle')}</p>
        {!IS_MOBILE && <span className="h-px w-6 bg-[var(--border-strong)]" />}
      </motion.div>
    </div>
  );
}

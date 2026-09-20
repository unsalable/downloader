import { motion } from 'motion/react';
import { ListChecks, ShieldCheck, Sparkles } from 'lucide-react';

import { Logo } from '@/components/layout/Logo';
import { Button } from '@/components/ui/Button';
import { useTranslation } from '@/i18n';
import { SPRING, T, stagger } from '@/lib/motion';
import type { TranslationKey } from '@/i18n';

const POINTS: { icon: typeof ShieldCheck; title: TranslationKey; body: TranslationKey }[] = [
  { icon: ShieldCheck, title: 'welcome.pointPrivacy', body: 'welcome.pointPrivacyBody' },
  { icon: Sparkles, title: 'welcome.pointQuality', body: 'welcome.pointQualityBody' },
  { icon: ListChecks, title: 'welcome.pointQueue', body: 'welcome.pointQueueBody' },
];

/**
 * Deliberately one screen, not a multi-step tour: the product is "paste a link",
 * and anything longer stands between the user and doing that.
 */
export function WelcomeScreen({ onStart }: { onStart: () => void }) {
  const { t } = useTranslation();

  return (
    <div className="fixed inset-0 z-[900] flex items-center justify-center bg-bg p-8">
      <div aria-hidden="true" className="pointer-events-none absolute inset-0 overflow-hidden">
        <div className="absolute left-1/2 top-1/3 size-[52vw] min-h-[420px] min-w-[420px] -translate-x-1/2 -translate-y-1/2 rounded-full bg-[radial-gradient(circle,var(--accent)_0%,transparent_66%)] opacity-[0.14] blur-[110px]" />
      </div>

      <motion.div
        initial={{ opacity: 0, y: 16, scale: 0.98 }}
        animate={{ opacity: 1, y: 0, scale: 1 }}
        transition={T.entrance}
        className="relative w-full max-w-[420px] text-center"
      >
        <motion.div
          initial={{ scale: 0.7, opacity: 0 }}
          animate={{ scale: 1, opacity: 1 }}
          transition={{ ...SPRING.settle, delay: 0.1 }}
          className="mx-auto w-fit"
        >
          <Logo size={58} />
        </motion.div>

        <h1 className="mt-6 text-[22px] font-semibold tracking-[-0.025em] text-fg">
          {t('welcome.title')}
        </h1>
        <p className="mt-1.5 text-[14px] text-fg-muted">{t('welcome.subtitle')}</p>

        <div className="mt-8 space-y-3 text-left">
          {POINTS.map((point, index) => {
            const Icon = point.icon;
            return (
              <motion.div
                key={point.title}
                initial={{ opacity: 0, x: -10 }}
                animate={{ opacity: 1, x: 0 }}
                transition={{ ...T.spatial, delay: 0.22 + stagger(index, 0.08) }}
                className="flex gap-3 rounded-[var(--radius-card)] border border-[var(--border)] bg-surface p-3.5"
              >
                <span className="flex size-8 shrink-0 items-center justify-center rounded-lg bg-accent-soft text-accent">
                  <Icon size={16} />
                </span>
                <div>
                  <div className="text-[13.5px] font-medium text-fg">{t(point.title)}</div>
                  <p className="mt-0.5 text-[12.5px] leading-relaxed text-fg-muted">
                    {t(point.body)}
                  </p>
                </div>
              </motion.div>
            );
          })}
        </div>

        <motion.div
          initial={{ opacity: 0 }}
          animate={{ opacity: 1 }}
          transition={{ ...T.spatial, delay: 0.5 }}
          className="mt-8"
        >
          <Button variant="cta" size="lg" fullWidth onClick={onStart} data-autofocus>
            {t('welcome.getStarted')}
          </Button>
        </motion.div>
      </motion.div>
    </div>
  );
}

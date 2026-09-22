import { Logo } from '@/components/layout/Logo';
import { Button } from '@/components/ui/Button';
import { ListGroup } from '@/components/ui/ListGroup';
import { useTranslation } from '@/i18n';
import type { TranslationKey } from '@/i18n';
import { cn } from '@/lib/cn';
import { IS_MOBILE } from '@/lib/platform';

const POINTS: { title: TranslationKey; body: TranslationKey }[] = [
  { title: 'welcome.pointPrivacy', body: 'welcome.pointPrivacyBody' },
  { title: 'welcome.pointQuality', body: 'welcome.pointQualityBody' },
  { title: 'welcome.pointQueue', body: 'welcome.pointQueueBody' },
];

/**
 * Deliberately one screen, not a multi-step tour: the product is "paste a link",
 * and anything longer stands between the user and doing that. It stands still
 * for the same reason -- nothing on it is an answer to something the user did.
 */
export function WelcomeScreen({ onStart }: { onStart: () => void }) {
  const { t } = useTranslation();

  return (
    <div
      className={cn(
        'fixed inset-0 z-[900] flex overflow-y-auto bg-bg',
        IS_MOBILE ? 'p-6' : 'p-8',
      )}
    >
      {/* Centred by its margins rather than by the container, so that on a
          short screen the top stays reachable instead of being cut off. */}
      <div className="m-auto w-full max-w-[400px] text-center">
        <Logo size={56} className="mx-auto" />

        <h1 className="mt-6 text-balance text-[26px] font-semibold leading-[1.2] tracking-[-0.022em] text-fg">
          {t('welcome.title')}
        </h1>
        <p className="mt-2 text-[14px] text-fg-muted">{t('welcome.subtitle')}</p>

        <ListGroup className="mt-8 text-left">
          {POINTS.map((point) => (
            <div key={point.title} className={cn('px-4', IS_MOBILE ? 'py-3.5' : 'py-3')}>
              <div className={cn('text-fg', IS_MOBILE ? 'text-[15px]' : 'text-[13.5px]')}>
                {t(point.title)}
              </div>
              <p
                className={cn(
                  'mt-0.5 leading-relaxed text-fg-muted',
                  IS_MOBILE ? 'text-[13px]' : 'text-[12.5px]',
                )}
              >
                {t(point.body)}
              </p>
            </div>
          ))}
        </ListGroup>

        <Button variant="cta" size="lg" fullWidth className="mt-8" onClick={onStart}>
          {t('welcome.getStarted')}
        </Button>
      </div>
    </div>
  );
}

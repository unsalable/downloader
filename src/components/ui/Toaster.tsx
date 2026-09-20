import { AnimatePresence, motion } from 'motion/react';
import { AlertTriangle, CheckCircle2, Info, X, XCircle } from 'lucide-react';

import { useTranslation } from '@/i18n';
import { cn } from '@/lib/cn';
import { SPRING, T } from '@/lib/motion';
import { IS_MOBILE } from '@/lib/platform';
import { useToastStore, type ToastTone } from '@/stores/useToastStore';

const ICONS: Record<ToastTone, typeof Info> = {
  info: Info,
  success: CheckCircle2,
  error: XCircle,
  warning: AlertTriangle,
};

const TONE_CLASS: Record<ToastTone, string> = {
  info: 'text-accent',
  success: 'text-success',
  error: 'text-error',
  warning: 'text-warning',
};

/**
 * A toast enters and leaves along the axis it is docked on: sideways from the
 * right on the desktop, upward from the bottom edge on a phone. Sliding a
 * bottom-anchored card off to the right would be motion pointing at nothing.
 */
const ENTER = IS_MOBILE ? { y: 24, x: 0 } : { x: 32, y: 0 };
const LEAVE = IS_MOBILE ? { y: 16, x: 0 } : { x: 24, y: 0 };

export function Toaster() {
  const { t } = useTranslation();
  const toasts = useToastStore((state) => state.toasts);
  const dismiss = useToastStore((state) => state.dismiss);
  const pause = useToastStore((state) => state.pause);
  const resume = useToastStore((state) => state.resume);

  return (
    <div
      // Non-interactive wrapper so the column never blocks clicks on the app
      // behind it; each card re-enables pointer events for itself.
      className={cn(
        'pointer-events-none fixed z-[950] flex flex-col gap-2.5',
        // On a phone the column spans the screen, clear of the bottom tabs.
        IS_MOBILE ? 'inset-x-3 bottom-[74px]' : 'bottom-5 right-5 w-[340px]',
      )}
      role="region"
      aria-live="polite"
      aria-label={t('toast.region')}
      // Reading a toast, or tabbing into one, holds every countdown. A message
      // with a button on it asks to be acted on, and the reaching for it is
      // exactly when the clock used to run out.
      onPointerEnter={pause}
      onPointerLeave={resume}
      onFocusCapture={pause}
      onBlurCapture={resume}
    >
      <AnimatePresence initial={false}>
        {toasts.map((item) => {
          const Icon = ICONS[item.tone];
          return (
            <motion.div
              key={item.id}
              layout
              // A failure interrupts; everything else waits its turn in the
              // surrounding polite region.
              role={item.tone === 'error' ? 'alert' : undefined}
              initial={{ opacity: 0, scale: 0.96, ...ENTER }}
              animate={{ opacity: 1, scale: 1, x: 0, y: 0 }}
              exit={{ opacity: 0, scale: 0.96, ...LEAVE, transition: T.componentOut }}
              transition={SPRING.settle}
              className={cn(
                'pointer-events-auto flex gap-3 rounded-[var(--radius-card)] p-3.5',
                'border border-[var(--border-strong)] bg-surface shadow-floating glass edge-light',
              )}
            >
              <Icon size={17} className={cn('mt-px shrink-0', TONE_CLASS[item.tone])} />
              <div className="min-w-0 flex-1">
                <div className="text-[13.5px] font-semibold leading-snug text-fg">{item.title}</div>
                {item.body && (
                  <p className="mt-0.5 break-words text-[12.5px] leading-relaxed text-fg-muted">
                    {item.body}
                  </p>
                )}
                {item.actions && item.actions.length > 0 && (
                  <div className="mt-2.5 flex items-center gap-2">
                    {item.actions.map((action) => (
                      <button
                        key={action.label}
                        type="button"
                        onClick={() => {
                          action.onClick();
                          dismiss(item.id);
                        }}
                        className={cn(
                          'pressable h-7 rounded-lg px-2.5 text-[12.5px] font-medium',
                          action.primary
                            ? 'bg-accent text-accent-fg hover:bg-accent-hover'
                            : 'text-fg-muted hover:bg-surface-hover hover:text-fg',
                        )}
                      >
                        {action.label}
                      </button>
                    ))}
                  </div>
                )}
              </div>
              <button
                type="button"
                aria-label={t('toast.dismiss')}
                onClick={() => dismiss(item.id)}
                className="pressable-sm -mr-1 -mt-1 size-6 shrink-0 rounded-md text-fg-faint hover:bg-surface-hover hover:text-fg"
              >
                <X size={13} className="mx-auto" />
              </button>
            </motion.div>
          );
        })}
      </AnimatePresence>
    </div>
  );
}

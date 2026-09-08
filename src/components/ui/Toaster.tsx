import { AnimatePresence, motion } from 'motion/react';
import { AlertTriangle, CheckCircle2, Info, X, XCircle } from 'lucide-react';

import { cn } from '@/lib/cn';
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

export function Toaster() {
  const toasts = useToastStore((state) => state.toasts);
  const dismiss = useToastStore((state) => state.dismiss);

  return (
    <div
      // Non-interactive wrapper so the column never blocks clicks on the app
      // behind it; each card re-enables pointer events for itself.
      className="pointer-events-none fixed bottom-5 right-5 z-[950] flex w-[340px] flex-col gap-2.5"
      role="region"
      aria-live="polite"
    >
      <AnimatePresence initial={false}>
        {toasts.map((item) => {
          const Icon = ICONS[item.tone];
          return (
            <motion.div
              key={item.id}
              layout
              initial={{ opacity: 0, x: 32, scale: 0.96 }}
              animate={{ opacity: 1, x: 0, scale: 1 }}
              exit={{ opacity: 0, x: 24, scale: 0.96, transition: { duration: 0.15 } }}
              transition={{ type: 'spring', stiffness: 420, damping: 34, mass: 0.8 }}
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
                          'h-7 rounded-lg px-2.5 text-[12.5px] font-medium transition-colors duration-150',
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
                aria-label="Dismiss"
                onClick={() => dismiss(item.id)}
                className="-mr-1 -mt-1 size-6 shrink-0 rounded-md text-fg-faint transition-colors hover:bg-surface-hover hover:text-fg"
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

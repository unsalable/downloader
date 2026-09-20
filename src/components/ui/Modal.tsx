import { AnimatePresence, motion } from 'motion/react';
import { X } from 'lucide-react';
import { useEffect, useId, useRef, type ReactNode } from 'react';
import { createPortal } from 'react-dom';

import { cn } from '@/lib/cn';
import { DIALOG, T } from '@/lib/motion';
import { IconButton } from './IconButton';

interface ModalProps {
  open: boolean;
  onClose: () => void;
  title?: ReactNode;
  description?: ReactNode;
  children?: ReactNode;
  footer?: ReactNode;
  width?: number;
  closeLabel?: string;
}

export function Modal({
  open,
  onClose,
  title,
  description,
  children,
  footer,
  width = 460,
  closeLabel = 'Close',
}: ModalProps) {
  const panelRef = useRef<HTMLDivElement>(null);
  const restoreFocusRef = useRef<HTMLElement | null>(null);
  const titleId = useId();
  const descriptionId = useId();

  useEffect(() => {
    if (!open) return;

    restoreFocusRef.current = document.activeElement as HTMLElement | null;

    // Stop the page behind the scrim from scrolling under the wheel. Without
    // this the dialog stays put while the screen it covers slides about, which
    // reads as two things happening at once rather than one thing on top of
    // another. The scrollbar it hides is replaced by padding, so the layout
    // does not jump sideways as the dialog opens.
    const { body, documentElement } = document;
    const previousOverflow = body.style.overflow;
    const previousPadding = body.style.paddingRight;
    const scrollbar = window.innerWidth - documentElement.clientWidth;
    body.style.overflow = 'hidden';
    if (scrollbar > 0) body.style.paddingRight = `${scrollbar}px`;

    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === 'Escape') {
        event.preventDefault();
        onClose();
        return;
      }
      if (event.key !== 'Tab') return;

      // Keep Tab inside the dialog: without this the focus ring wanders into
      // the (inert-looking but still focusable) page behind the scrim.
      const focusables = panelRef.current?.querySelectorAll<HTMLElement>(
        'button:not([disabled]), [href], input:not([disabled]), select, textarea, [tabindex]:not([tabindex="-1"])',
      );
      if (!focusables || focusables.length === 0) return;
      const first = focusables[0]!;
      const last = focusables[focusables.length - 1]!;
      if (event.shiftKey && document.activeElement === first) {
        event.preventDefault();
        last.focus();
      } else if (!event.shiftKey && document.activeElement === last) {
        event.preventDefault();
        first.focus();
      }
    };

    document.addEventListener('keydown', onKeyDown);
    const timer = window.setTimeout(() => {
      panelRef.current?.querySelector<HTMLElement>('[data-autofocus]')?.focus() ??
        panelRef.current?.focus();
    }, 40);

    return () => {
      document.removeEventListener('keydown', onKeyDown);
      window.clearTimeout(timer);
      body.style.overflow = previousOverflow;
      body.style.paddingRight = previousPadding;
      restoreFocusRef.current?.focus?.();
    };
  }, [open, onClose]);

  return createPortal(
    <AnimatePresence>
      {open && (
        <div className="fixed inset-0 z-[800] flex items-center justify-center p-6">
          <motion.div
            initial={{ opacity: 0 }}
            animate={{ opacity: 1 }}
            exit={{ opacity: 0, transition: T.spatialOut }}
            transition={T.spatial}
            onClick={onClose}
            className="absolute inset-0 bg-scrim backdrop-blur-[3px]"
          />
          <motion.div
            ref={panelRef}
            role="dialog"
            aria-modal="true"
            aria-labelledby={title ? titleId : undefined}
            aria-describedby={description ? descriptionId : undefined}
            tabIndex={-1}
            variants={DIALOG}
            initial="initial"
            animate="animate"
            exit="exit"
            // Never wider than the screen: a phone is narrower than most dialogs.
            style={{ width, maxWidth: '100%' }}
            className={cn(
              'relative max-h-[85vh] overflow-hidden rounded-[var(--radius-panel)]',
              'border border-[var(--border-strong)] bg-surface shadow-floating edge-light',
              'flex flex-col',
            )}
          >
            {(title || description) && (
              <header className="flex items-start gap-3 px-5 pb-3 pt-5">
                <div className="min-w-0 flex-1">
                  {title && (
                    <h2 id={titleId} className="text-[16px] font-semibold text-fg">
                      {title}
                    </h2>
                  )}
                  {description && (
                    <p id={descriptionId} className="mt-1 text-[13px] leading-relaxed text-fg-muted">
                      {description}
                    </p>
                  )}
                </div>
                <IconButton
                  icon={<X size={16} />}
                  label={closeLabel}
                  onClick={onClose}
                  showTooltip={false}
                  className="-mr-1 -mt-1"
                />
              </header>
            )}

            {children && <div className="min-h-0 flex-1 overflow-y-auto px-5 py-1">{children}</div>}

            {footer && (
              <footer className="flex items-center justify-end gap-2 border-t border-[var(--border)] px-5 py-3.5">
                {footer}
              </footer>
            )}
          </motion.div>
        </div>
      )}
    </AnimatePresence>,
    document.body,
  );
}

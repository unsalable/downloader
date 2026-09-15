import { AnimatePresence, motion } from 'motion/react';
import {
  cloneElement,
  useCallback,
  useId,
  useRef,
  useState,
  type ReactElement,
  type ReactNode,
} from 'react';
import { createPortal } from 'react-dom';

import { cn } from '@/lib/cn';
import { IS_MOBILE } from '@/lib/platform';

type Side = 'top' | 'bottom' | 'left' | 'right';

interface TooltipProps {
  label: ReactNode;
  side?: Side;
  delayMs?: number;
  children: ReactElement<Record<string, unknown>>;
}

/**
 * Portal-rendered tooltip positioned from the trigger's own rect. Position is
 * measured on open only -- tooltips are short-lived, so tracking scroll would
 * cost more than it is worth.
 */
export function Tooltip({ label, side = 'top', delayMs = 380, children }: TooltipProps) {
  const [coords, setCoords] = useState<{ x: number; y: number } | null>(null);
  const triggerRef = useRef<HTMLElement | null>(null);
  const timerRef = useRef<number | null>(null);
  const id = useId();

  const clearTimer = useCallback(() => {
    if (timerRef.current !== null) {
      window.clearTimeout(timerRef.current);
      timerRef.current = null;
    }
  }, []);

  const open = useCallback(
    (element: HTMLElement) => {
      clearTimer();
      timerRef.current = window.setTimeout(() => {
        const rect = element.getBoundingClientRect();
        const gap = 8;
        const position = {
          top: { x: rect.left + rect.width / 2, y: rect.top - gap },
          bottom: { x: rect.left + rect.width / 2, y: rect.bottom + gap },
          left: { x: rect.left - gap, y: rect.top + rect.height / 2 },
          right: { x: rect.right + gap, y: rect.top + rect.height / 2 },
        }[side];
        setCoords(position);
      }, delayMs);
    },
    [clearTimer, delayMs, side],
  );

  const close = useCallback(() => {
    clearTimer();
    setCoords(null);
  }, [clearTimer]);

  // A touch screen has no hover, and a tap leaves focus behind, which would
  // pin the tooltip open over whatever was tapped.
  if (IS_MOBILE) return children;

  const trigger = cloneElement(children, {
    ref: (node: HTMLElement | null) => {
      triggerRef.current = node;
      const originalRef = (children as unknown as { ref?: unknown }).ref;
      if (typeof originalRef === 'function') originalRef(node);
      else if (originalRef && typeof originalRef === 'object') {
        (originalRef as { current: HTMLElement | null }).current = node;
      }
    },
    onMouseEnter: (event: React.MouseEvent<HTMLElement>) => {
      open(event.currentTarget);
      (children.props.onMouseEnter as ((e: React.MouseEvent) => void) | undefined)?.(event);
    },
    onMouseLeave: (event: React.MouseEvent<HTMLElement>) => {
      close();
      (children.props.onMouseLeave as ((e: React.MouseEvent) => void) | undefined)?.(event);
    },
    onFocus: (event: React.FocusEvent<HTMLElement>) => {
      open(event.currentTarget);
      (children.props.onFocus as ((e: React.FocusEvent) => void) | undefined)?.(event);
    },
    onBlur: (event: React.FocusEvent<HTMLElement>) => {
      close();
      (children.props.onBlur as ((e: React.FocusEvent) => void) | undefined)?.(event);
    },
    'aria-describedby': coords ? id : undefined,
  });

  const translate: Record<Side, string> = {
    top: 'translate(-50%, -100%)',
    bottom: 'translate(-50%, 0)',
    left: 'translate(-100%, -50%)',
    right: 'translate(0, -50%)',
  };

  const offset: Record<Side, { x: number; y: number }> = {
    top: { x: 0, y: 4 },
    bottom: { x: 0, y: -4 },
    left: { x: 4, y: 0 },
    right: { x: -4, y: 0 },
  };

  return (
    <>
      {trigger}
      {createPortal(
        <AnimatePresence>
          {coords && (
            <motion.div
              id={id}
              role="tooltip"
              initial={{ opacity: 0, ...offset[side], scale: 0.96 }}
              animate={{ opacity: 1, x: 0, y: 0, scale: 1 }}
              exit={{ opacity: 0, scale: 0.96, transition: { duration: 0.1 } }}
              transition={{ duration: 0.16, ease: [0.22, 1, 0.36, 1] }}
              style={{ left: coords.x, top: coords.y, transform: translate[side] }}
              className={cn(
                'pointer-events-none fixed z-[999] max-w-56 rounded-lg px-2.5 py-1.5',
                'bg-[var(--surface-active)] text-[12px] font-medium text-fg',
                'border border-[var(--border-strong)] shadow-floating',
              )}
            >
              {label}
            </motion.div>
          )}
        </AnimatePresence>,
        document.body,
      )}
    </>
  );
}

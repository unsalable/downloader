import { AnimatePresence, motion } from 'motion/react';
import {
  cloneElement,
  useCallback,
  useId,
  useLayoutEffect,
  useRef,
  useState,
  type ReactElement,
  type ReactNode,
} from 'react';
import { createPortal } from 'react-dom';

import { cn } from '@/lib/cn';
import { T } from '@/lib/motion';
import { IS_MOBILE } from '@/lib/platform';

type Side = 'top' | 'bottom' | 'left' | 'right';

interface TooltipProps {
  label: ReactNode;
  side?: Side;
  delayMs?: number;
  children: ReactElement<Record<string, unknown>>;
}

/** How the box is hung off its anchor point. */
const ANCHOR: Record<Side, string> = {
  top: 'translate(-50%, -100%)',
  bottom: 'translate(-50%, 0)',
  left: 'translate(-100%, -50%)',
  right: 'translate(0, -50%)',
};

/** The corner the box grows out of, which is the side facing the trigger. */
const ORIGIN: Record<Side, string> = {
  top: 'bottom center',
  bottom: 'top center',
  left: 'right center',
  right: 'left center',
};

/** A short slide toward the trigger, so the box reads as coming from it. */
const OFFSET: Record<Side, { x: number; y: number }> = {
  top: { x: 0, y: 2 },
  bottom: { x: 0, y: -2 },
  left: { x: 2, y: 0 },
  right: { x: -2, y: 0 },
};

/** Kept clear of the window edge. */
const MARGIN = 8;

/** The gap between the trigger and the box. */
const GAP = 8;

/**
 * Room a tooltip needs on the side it prefers, before it gives up and goes to
 * the other one. Two lines of the box plus the gap, which is as tall as these
 * get; being generous only means flipping a little early, which is harmless,
 * while being mean means a box that lands on top of the thing it describes.
 */
const CLEARANCE = { y: 56, x: 180 };

/** The side with room on it, which is not always the side that was asked for. */
function resolveSide(side: Side, rect: DOMRect): Side {
  if (side === 'top') return rect.top < CLEARANCE.y ? 'bottom' : 'top';
  if (side === 'bottom') {
    return window.innerHeight - rect.bottom < CLEARANCE.y ? 'top' : 'bottom';
  }
  if (side === 'left') return rect.left < CLEARANCE.x ? 'right' : 'left';
  return window.innerWidth - rect.right < CLEARANCE.x ? 'left' : 'right';
}

/**
 * Portal-rendered tooltip positioned from the trigger's own rect.
 *
 * Positioning and animation are deliberately split across two elements. The
 * outer one is never animated and owns `left`/`top` and the anchoring
 * translate; the inner one owns opacity and scale. Putting both on one element
 * does not work -- Motion composes the element's `transform` from the values it
 * animates, so an anchoring translate written in `style` is overwritten on the
 * first frame and the box lands at the corner of its anchor instead of over it.
 *
 * Position is measured on open only. Tooltips are short-lived, so tracking
 * scroll would cost more than it is worth; what is worth doing is nudging the
 * box back inside the window when the trigger sits near an edge, which is what
 * the layout effect below does before the first paint.
 */
export function Tooltip({ label, side = 'top', delayMs = 380, children }: TooltipProps) {
  const [coords, setCoords] = useState<{ x: number; y: number; side: Side } | null>(null);
  const triggerRef = useRef<HTMLElement | null>(null);
  const frameRef = useRef<HTMLDivElement>(null);
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
        const placed = resolveSide(side, rect);
        const anchor = {
          top: { x: rect.left + rect.width / 2, y: rect.top - GAP },
          bottom: { x: rect.left + rect.width / 2, y: rect.bottom + GAP },
          left: { x: rect.left - GAP, y: rect.top + rect.height / 2 },
          right: { x: rect.right + GAP, y: rect.top + rect.height / 2 },
        }[placed];
        setCoords({ ...anchor, side: placed });
      }, delayMs);
    },
    [clearTimer, delayMs, side],
  );

  const close = useCallback(() => {
    clearTimer();
    setCoords(null);
  }, [clearTimer]);

  // Slide the box back inside the window if the anchor put it over an edge.
  // Written straight to the node rather than through state: this runs before
  // paint, so there is nothing to re-render and nothing to see move.
  useLayoutEffect(() => {
    const node = frameRef.current;
    if (!node || !coords) return;

    node.style.left = `${coords.x}px`;
    node.style.top = `${coords.y}px`;

    const rect = node.getBoundingClientRect();
    if (rect.width === 0) return;

    const shift = (near: number, far: number, limit: number) =>
      near < MARGIN ? MARGIN - near : far > limit - MARGIN ? limit - MARGIN - far : 0;

    const dx = shift(rect.left, rect.right, window.innerWidth);
    const dy = shift(rect.top, rect.bottom, window.innerHeight);
    if (dx !== 0) node.style.left = `${coords.x + dx}px`;
    if (dy !== 0) node.style.top = `${coords.y + dy}px`;
  }, [coords]);

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

  return (
    <>
      {trigger}
      {createPortal(
        <AnimatePresence>
          {coords && (
            <div
              ref={frameRef}
              className="pointer-events-none fixed z-[999]"
              style={{ left: coords.x, top: coords.y, transform: ANCHOR[coords.side] }}
            >
              <motion.div
                id={id}
                role="tooltip"
                initial={{ opacity: 0, scale: 0.98, ...OFFSET[coords.side] }}
                animate={{ opacity: 1, scale: 1, x: 0, y: 0 }}
                exit={{ opacity: 0, scale: 0.98, transition: T.microOut }}
                transition={T.micro}
                style={{ transformOrigin: ORIGIN[coords.side] }}
                className={cn(
                  'max-w-56 rounded-[8px] px-2.5 py-1.5 text-[12px] font-medium text-fg',
                  'border border-[var(--border)] bg-surface shadow-raised',
                  'dark:border-[var(--border-strong)] dark:bg-surface-active',
                )}
              >
                {label}
              </motion.div>
            </div>
          )}
        </AnimatePresence>,
        document.body,
      )}
    </>
  );
}

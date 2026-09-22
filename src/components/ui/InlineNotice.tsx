import { CircleAlert, CircleCheck, Info, type LucideIcon } from 'lucide-react';
import type { ReactNode } from 'react';

import { cn } from '@/lib/cn';

/*
 * A line of feedback set where the action happened -- under the field that was
 * wrong, beside the button that failed. There are no popups in the app; this is
 * what says so instead.
 *
 *   {error && <InlineNotice tone="error">{error}</InlineNotice>}
 *
 * Render it only while there is something to say: an error is announced when
 * the element appears, not when its text changes from empty.
 */

type NoticeTone = 'error' | 'success' | 'info';

const TONES: Record<NoticeTone, { icon: LucideIcon; className: string }> = {
  error: { icon: CircleAlert, className: 'text-error' },
  success: { icon: CircleCheck, className: 'text-success' },
  info: { icon: Info, className: 'text-fg-muted' },
};

interface InlineNoticeProps {
  tone?: NoticeTone;
  children: ReactNode;
  className?: string;
}

export function InlineNotice({ tone = 'info', children, className }: InlineNoticeProps) {
  const { icon: Icon, className: toneClass } = TONES[tone];

  return (
    <p
      // An error interrupts a screen reader; a confirmation waits its turn.
      role={tone === 'error' ? 'alert' : 'status'}
      className={cn('flex items-start gap-1.5 text-[12.5px] leading-[1.4]', toneClass, className)}
    >
      {/* Nudged down so it sits on the first line's x-height, not above it. */}
      <Icon size={14} aria-hidden="true" className="mt-[1.5px] shrink-0" />
      <span className="selectable min-w-0">{children}</span>
    </p>
  );
}

import type { ReactNode } from 'react';

import { cn } from '@/lib/cn';

export type BadgeTone = 'neutral' | 'accent' | 'success' | 'warning' | 'error' | 'outline';

const TONES: Record<BadgeTone, string> = {
  neutral: 'bg-[var(--surface-active)] text-fg-muted',
  accent: 'bg-accent-soft text-accent',
  success: 'bg-success-soft text-success',
  warning: 'bg-warning-soft text-warning',
  error: 'bg-error-soft text-error',
  outline: 'border border-[var(--border-strong)] text-fg-muted',
};

export function Badge({
  children,
  tone = 'neutral',
  icon,
  className,
}: {
  children: ReactNode;
  tone?: BadgeTone;
  icon?: ReactNode;
  className?: string;
}) {
  return (
    <span
      className={cn(
        'inline-flex items-center gap-1 rounded-[4px] px-1.5 py-0.5',
        // Small, spaced and upper case: a status stamp, not a pill.
        'font-mono text-[10px] font-semibold uppercase leading-[17px] tracking-[0.07em]',
        TONES[tone],
        className,
      )}
    >
      {icon}
      {children}
    </span>
  );
}

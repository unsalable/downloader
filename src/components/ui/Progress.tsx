import { cn } from '@/lib/cn';

interface ProgressProps {
  /** 0..100. Pass null for an indeterminate bar. */
  value: number | null;
  tone?: 'accent' | 'success' | 'error' | 'muted';
  size?: 'sm' | 'md';
  className?: string;
  label?: string;
}

const TONES = {
  accent: 'bg-[var(--accent)]',
  success: 'bg-[var(--success)]',
  error: 'bg-[var(--error)]',
  muted: 'bg-[var(--text-tertiary)]',
} as const;

/**
 * Determinate fill is animated with a CSS transition rather than a spring: the
 * backend emits progress a few times a second, and a linear interpolation
 * between those samples is what makes the bar look continuous. A spring would
 * overshoot backwards whenever a tick arrived slightly late.
 */
export function Progress({ value, tone = 'accent', size = 'md', className, label }: ProgressProps) {
  const indeterminate = value == null;

  return (
    <div
      role="progressbar"
      aria-label={label}
      aria-valuemin={0}
      aria-valuemax={100}
      aria-valuenow={indeterminate ? undefined : Math.round(value)}
      className={cn(
        'relative w-full overflow-hidden rounded-[2px] bg-[var(--surface-active)]',
        size === 'sm' ? 'h-1' : 'h-[5px]',
        className,
      )}
    >
      {indeterminate ? (
        <div
          className={cn(
            'absolute inset-y-0 w-2/5 rounded-[2px] [animation:ud-sweep_1.25s_ease-in-out_infinite]',
            TONES[tone],
          )}
        />
      ) : (
        <div
          className={cn('h-full rounded-[2px] transition-[width] duration-300 ease-linear', TONES[tone])}
          style={{ width: `${Math.min(100, Math.max(0, value))}%` }}
        />
      )}
    </div>
  );
}

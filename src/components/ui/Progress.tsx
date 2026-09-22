import { cn } from '@/lib/cn';

interface ProgressProps {
  /** 0..100. Pass null for an indeterminate bar. */
  value: number | null;
  className?: string;
  label?: string;
}

/**
 * Determinate fill is animated with a CSS transition rather than a spring: the
 * backend emits progress a few times a second, and a linear interpolation
 * between those samples is what makes the bar look continuous. A spring would
 * overshoot backwards whenever a tick arrived slightly late.
 */
export function Progress({ value, className, label }: ProgressProps) {
  const indeterminate = value == null;

  return (
    <div
      role="progressbar"
      aria-label={label}
      aria-valuemin={0}
      aria-valuemax={100}
      aria-valuenow={indeterminate ? undefined : Math.round(value)}
      className={cn('relative h-[3px] w-full overflow-hidden rounded-full bg-fill-hover', className)}
    >
      {indeterminate ? (
        <div
          className={cn(
            'motion-essential absolute inset-y-0 w-2/5 rounded-full bg-accent',
            '[animation:ud-sweep_1.25s_ease-in-out_infinite]',
          )}
        />
      ) : (
        // Scaled rather than resized: a width transition lays the page out
        // again on every frame it runs, while a transform stays on the
        // compositor. With several downloads ticking that difference is felt.
        <div
          className={cn(
            // Exempt from the reduce-motion rule for the same reason the
            // indeterminate bar is: the interpolation is not decoration, it is
            // how a sample taken three times a second is drawn as a rate. Frozen,
            // the bar steps, and a stepping bar is harder to read, not calmer.
            'motion-essential h-full w-full origin-left',
            'bg-accent transition-transform duration-300 ease-linear',
          )}
          style={{ transform: `scaleX(${Math.min(100, Math.max(0, value)) / 100})` }}
        />
      )}
    </div>
  );
}

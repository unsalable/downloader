import { motion } from 'motion/react';
import { useId, type ReactNode } from 'react';

import { cn } from '@/lib/cn';
import { IS_MOBILE } from '@/lib/platform';

export interface SegmentedOption<T extends string> {
  value: T;
  label: string;
  icon?: ReactNode;
  disabled?: boolean;
}

interface SegmentedProps<T extends string> {
  value: T;
  options: SegmentedOption<T>[];
  onChange: (value: T) => void;
  label?: string;
  size?: 'sm' | 'md';
  className?: string;
}

/**
 * Segmented control with a shared layout indicator: the highlight is a single
 * element that animates between segments instead of each segment fading, which
 * reads as one object moving rather than two crossfading.
 */
export function Segmented<T extends string>({
  value,
  options,
  onChange,
  label,
  size = 'md',
  className,
}: SegmentedProps<T>) {
  const layoutId = useId();

  return (
    <div className={cn('flex flex-col gap-1.5', className)}>
      {label && (
        <span className="text-[11px] font-medium uppercase tracking-[0.07em] text-fg-faint">
          {label}
        </span>
      )}
      <div
        role="radiogroup"
        aria-label={label}
        className={cn(
          'inline-flex w-full items-center gap-1 rounded-[10px] border border-[var(--border)]',
          'bg-[var(--surface-sunken)] p-1',
        )}
      >
        {options.map((option) => {
          const selected = option.value === value;
          return (
            <button
              key={option.value}
              type="button"
              role="radio"
              aria-checked={selected}
              disabled={option.disabled}
              onClick={() => onChange(option.value)}
              className={cn(
                'relative flex flex-1 items-center justify-center gap-1.5 rounded-[7px] font-medium',
                'transition-colors duration-150 disabled:pointer-events-none disabled:opacity-40',
                IS_MOBILE
                  ? 'h-10 px-2 text-[14px]'
                  : size === 'sm'
                    ? 'h-7 px-2 text-[12.5px]'
                    : 'h-8 px-3 text-[13px]',
                selected ? 'text-fg' : 'text-fg-muted hover:text-fg',
              )}
            >
              {selected && (
                <motion.span
                  layoutId={layoutId}
                  transition={{ type: 'spring', stiffness: 520, damping: 40, mass: 0.7 }}
                  className="absolute inset-0 rounded-[7px] bg-surface shadow-soft"
                />
              )}
              <span className="relative z-10 flex items-center gap-1.5">
                {option.icon}
                {option.label}
              </span>
            </button>
          );
        })}
      </div>
    </div>
  );
}

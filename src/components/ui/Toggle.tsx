import { motion } from 'motion/react';

import { cn } from '@/lib/cn';

interface ToggleProps {
  checked: boolean;
  onChange: (checked: boolean) => void;
  disabled?: boolean;
  label?: string;
  id?: string;
}

export function Toggle({ checked, onChange, disabled = false, label, id }: ToggleProps) {
  return (
    <button
      id={id}
      type="button"
      role="switch"
      aria-checked={checked}
      aria-label={label}
      disabled={disabled}
      onClick={() => onChange(!checked)}
      className={cn(
        'relative inline-flex h-[22px] w-[38px] shrink-0 items-center rounded-full px-[3px]',
        'transition-colors duration-200 disabled:pointer-events-none disabled:opacity-40',
        checked ? 'bg-accent' : 'bg-[var(--surface-active)]',
      )}
    >
      <motion.span
        layout
        // A spring here gives the knob a touch of overshoot, which is what makes
        // the control feel physical rather than merely animated.
        transition={{ type: 'spring', stiffness: 620, damping: 34, mass: 0.6 }}
        className={cn(
          'block size-4 rounded-full bg-white shadow-sm',
          checked ? 'ml-auto' : 'mr-auto',
        )}
      />
    </button>
  );
}

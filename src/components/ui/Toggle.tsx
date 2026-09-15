import { motion } from 'motion/react';

import { cn } from '@/lib/cn';
import { IS_MOBILE } from '@/lib/platform';

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
      className="shrink-0 rounded-full disabled:pointer-events-none disabled:opacity-40"
    >
      <ToggleTrack checked={checked} />
    </button>
  );
}

/**
 * The switch as drawn, with no behaviour of its own -- for a row that is itself
 * the button, where a second button inside would be invalid markup.
 */
export function ToggleTrack({ checked }: { checked: boolean }) {
  return (
    <span
      aria-hidden="true"
      className={cn(
        'relative flex shrink-0 items-center rounded-full px-[3px] transition-colors duration-200',
        IS_MOBILE ? 'h-[28px] w-[48px]' : 'h-[22px] w-[38px]',
        checked ? 'bg-accent' : 'bg-[var(--surface-active)]',
      )}
    >
      <motion.span
        layout
        // A spring here gives the knob a touch of overshoot, which is what makes
        // the control feel physical rather than merely animated.
        transition={{ type: 'spring', stiffness: 620, damping: 34, mass: 0.6 }}
        className={cn(
          'block rounded-full bg-white shadow-sm',
          IS_MOBILE ? 'size-[22px]' : 'size-4',
          checked ? 'ml-auto' : 'mr-auto',
        )}
      />
    </span>
  );
}

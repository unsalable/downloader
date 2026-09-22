import { motion } from 'motion/react';

import { cn } from '@/lib/cn';
import { SPRING } from '@/lib/motion';
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
      // No `pressable` here, unlike every other control: the knob inside is a
      // layout animation, and Motion measures it against an ancestor it does
      // not know is being scaled. The knob crossing the track is the feedback.
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
        // The proportions of the system switch: a track a little over one and a
        // half knobs wide, the knob filling it but for 2px all round.
        'relative flex shrink-0 items-center rounded-full p-[2px] transition-colors duration-250 ease-out-quint',
        IS_MOBILE ? 'h-[31px] w-[51px]' : 'h-[24px] w-[40px]',
        checked ? 'bg-accent' : 'bg-fill-active',
      )}
    >
      <motion.span
        layout
        // A spring rather than a duration, so a knob thrown across the track
        // comes to rest instead of stopping on a frame.
        transition={SPRING.snap}
        className={cn(
          'block rounded-full bg-white shadow-[0_0_0_0.5px_rgb(0_0_0/0.06),0_2px_5px_rgb(0_0_0/0.2)]',
          IS_MOBILE ? 'size-[27px]' : 'size-5',
          checked ? 'ml-auto' : 'mr-auto',
        )}
      />
    </span>
  );
}

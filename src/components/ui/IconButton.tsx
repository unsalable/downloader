import { forwardRef, type ButtonHTMLAttributes, type ReactNode } from 'react';

import { cn } from '@/lib/cn';
import { IS_MOBILE } from '@/lib/platform';
import { Tooltip } from './Tooltip';

export interface IconButtonProps extends ButtonHTMLAttributes<HTMLButtonElement> {
  icon: ReactNode;
  /** Required: this is the accessible name and the tooltip text. */
  label: string;
  size?: 'sm' | 'md';
  tone?: 'default' | 'accent' | 'danger';
  active?: boolean;
  showTooltip?: boolean;
}

export const IconButton = forwardRef<HTMLButtonElement, IconButtonProps>(function IconButton(
  {
    icon,
    label,
    size = 'md',
    tone = 'default',
    active = false,
    showTooltip = true,
    className,
    ...rest
  },
  ref,
) {
  const button = (
    <button
      ref={ref}
      type="button"
      aria-label={label}
      aria-pressed={active || undefined}
      className={cn(
        'pressable-sm inline-flex shrink-0 items-center justify-center rounded-[8px]',
        'disabled:pointer-events-none disabled:opacity-40',
        // A fingertip needs a larger target than a pointer does.
        size === 'sm' ? (IS_MOBILE ? 'size-9' : 'size-7') : IS_MOBILE ? 'size-10' : 'size-7.5',
        tone === 'default' && 'text-fg-muted hover:bg-fill hover:text-fg',
        tone === 'accent' && 'text-accent hover:bg-accent-soft',
        tone === 'danger' && 'text-fg-muted hover:bg-error-soft hover:text-error',
        active && 'bg-fill-active text-fg',
        className,
      )}
      {...rest}
    >
      {icon}
    </button>
  );

  return showTooltip ? <Tooltip label={label}>{button}</Tooltip> : button;
});

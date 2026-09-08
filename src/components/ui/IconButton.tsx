import { forwardRef, type ButtonHTMLAttributes, type ReactNode } from 'react';

import { cn } from '@/lib/cn';
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
        'inline-flex shrink-0 items-center justify-center rounded-md transition-all duration-150',
        'active:scale-[0.93] disabled:pointer-events-none disabled:opacity-40',
        size === 'sm' ? 'size-7' : 'size-8.5',
        tone === 'default' && 'text-fg-muted hover:bg-surface-hover hover:text-fg',
        tone === 'accent' && 'text-accent hover:bg-accent-soft',
        tone === 'danger' && 'text-fg-muted hover:bg-error-soft hover:text-error',
        active && 'bg-surface-active text-fg',
        className,
      )}
      {...rest}
    >
      {icon}
    </button>
  );

  return showTooltip ? <Tooltip label={label}>{button}</Tooltip> : button;
});

import { forwardRef, type ButtonHTMLAttributes, type ReactNode } from 'react';

import { cn } from '@/lib/cn';
import { IS_MOBILE } from '@/lib/platform';
import { Spinner } from './Spinner';

export type ButtonVariant = 'primary' | 'secondary' | 'ghost' | 'danger' | 'cta';
export type ButtonSize = 'sm' | 'md' | 'lg';

export interface ButtonProps extends ButtonHTMLAttributes<HTMLButtonElement> {
  variant?: ButtonVariant;
  size?: ButtonSize;
  loading?: boolean;
  icon?: ReactNode;
  iconRight?: ReactNode;
  fullWidth?: boolean;
}

const SIZES: Record<ButtonSize, string> = IS_MOBILE
  ? // Sized for a fingertip rather than a pointer.
    {
      sm: 'h-10 px-3.5 text-[13.5px] gap-1.5 rounded-lg',
      md: 'h-11 px-4 text-[14px] gap-2 rounded-[9px]',
      lg: 'h-13 px-6 text-[15px] gap-2.5 rounded-[10px]',
    }
  : {
      sm: 'h-8 px-3 text-[12.5px] gap-1.5 rounded-md',
      md: 'h-9.5 px-4 text-[13px] gap-2 rounded-[7px]',
      lg: 'h-12 px-6 text-[14.5px] gap-2.5 rounded-[9px]',
    };

const VARIANTS: Record<ButtonVariant, string> = {
  primary: 'bg-accent text-accent-fg hover:bg-accent-hover active:brightness-95',
  secondary:
    'bg-surface text-fg border border-[var(--border-strong)] hover:bg-surface-hover active:bg-surface-active',
  ghost: 'text-fg-muted hover:bg-surface-hover hover:text-fg',
  danger: 'bg-error-soft text-error hover:brightness-110 active:brightness-95',
  // The one call to action on a screen. It is the accent at full strength with
  // a warm cast beneath it -- the weight comes from the shadow, not from a
  // second colour sliding across the fill.
  cta: 'bg-accent text-accent-fg shadow-[0_6px_20px_-8px_var(--accent-ring)] hover:bg-accent-hover hover:shadow-[0_10px_28px_-8px_var(--accent-ring)] active:brightness-95',
};

export const Button = forwardRef<HTMLButtonElement, ButtonProps>(function Button(
  {
    variant = 'secondary',
    size = 'md',
    loading = false,
    icon,
    iconRight,
    fullWidth = false,
    className,
    children,
    disabled,
    ...rest
  },
  ref,
) {
  const isDisabled = disabled || loading;

  return (
    <button
      ref={ref}
      disabled={isDisabled}
      aria-busy={loading || undefined}
      className={cn(
        'pressable relative inline-flex select-none items-center justify-center font-medium',
        'disabled:pointer-events-none disabled:opacity-45',
        SIZES[size],
        VARIANTS[variant],
        fullWidth && 'w-full',
        className,
      )}
      {...rest}
    >
      {loading ? (
        <Spinner size={size === 'lg' ? 18 : 14} />
      ) : (
        icon && <span className="shrink-0 [&>svg]:block">{icon}</span>
      )}
      {children != null && <span className="truncate">{children}</span>}
      {iconRight && !loading && <span className="shrink-0 [&>svg]:block">{iconRight}</span>}
    </button>
  );
});

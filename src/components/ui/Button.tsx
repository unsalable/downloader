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
      sm: 'h-10 px-3.5 text-[13.5px] gap-1.5 rounded-[var(--radius-control)]',
      md: 'h-11 px-4 text-[14px] gap-2 rounded-[var(--radius-control)]',
      lg: 'h-13 px-6 text-[15px] gap-2.5 rounded-[var(--radius-card)]',
    }
  : {
      sm: 'h-8 px-3 text-[12.5px] gap-1.5 rounded-[8px]',
      md: 'h-9 px-4 text-[13px] gap-2 rounded-[var(--radius-control)]',
      lg: 'h-11 px-6 text-[14px] gap-2 rounded-[12px]',
    };

const ACCENT = 'bg-accent text-accent-fg hover:bg-accent-hover';

// Told apart by fill alone; none of them carries a border or a shadow.
const VARIANTS: Record<ButtonVariant, string> = {
  primary: ACCENT,
  secondary: 'bg-fill text-fg hover:bg-fill-hover active:bg-fill-active',
  ghost: 'text-fg-muted hover:bg-fill hover:text-fg active:bg-fill-hover',
  danger: 'bg-error-soft text-error hover:brightness-95 dark:hover:brightness-110',
  // The one call to action on a screen. Its size is what sets it apart; the
  // fill is the same accent as any other primary button.
  cta: ACCENT,
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

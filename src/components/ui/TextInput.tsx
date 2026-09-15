import { forwardRef, useId, type InputHTMLAttributes, type ReactNode } from 'react';

import { cn } from '@/lib/cn';
import { IS_MOBILE } from '@/lib/platform';

export interface TextInputProps extends InputHTMLAttributes<HTMLInputElement> {
  label?: string;
  hint?: ReactNode;
  error?: string | null;
  icon?: ReactNode;
  trailing?: ReactNode;
  monospace?: boolean;
}

export const TextInput = forwardRef<HTMLInputElement, TextInputProps>(function TextInput(
  { label, hint, error, icon, trailing, monospace, className, id, ...rest },
  ref,
) {
  const generatedId = useId();
  const inputId = id ?? generatedId;

  return (
    <div className="flex flex-col gap-1.5">
      {label && (
        <label htmlFor={inputId} className="text-[13px] font-medium text-fg">
          {label}
        </label>
      )}
      <div
        className={cn(
          'flex items-center gap-2 rounded-[10px] border bg-surface px-3',
          IS_MOBILE ? 'h-12' : 'h-10',
          'transition-all duration-150',
          'focus-within:border-[var(--accent)] focus-within:ring-2 focus-within:ring-[var(--accent-ring)]/30',
          error ? 'border-[var(--error)]' : 'border-[var(--border)] hover:border-[var(--border-strong)]',
          className,
        )}
      >
        {icon && <span className="shrink-0 text-fg-faint">{icon}</span>}
        <input
          ref={ref}
          id={inputId}
          aria-invalid={error ? true : undefined}
          className={cn(
            'min-w-0 flex-1 bg-transparent text-fg outline-none',
            'placeholder:text-fg-faint',
            monospace
              ? cn('font-mono', IS_MOBILE ? 'text-[13.5px]' : 'text-[12.5px]')
              : IS_MOBILE
                ? 'text-[15px]'
                : 'text-[13.5px]',
          )}
          {...rest}
        />
        {trailing}
      </div>
      {(error || hint) && (
        <p className={cn('text-[12px] leading-snug', error ? 'text-error' : 'text-fg-faint')}>
          {error ?? hint}
        </p>
      )}
    </div>
  );
});

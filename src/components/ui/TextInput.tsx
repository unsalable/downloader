import { forwardRef, useId, type InputHTMLAttributes, type ReactNode } from 'react';

import { cn } from '@/lib/cn';
import { IS_MOBILE } from '@/lib/platform';

export interface TextInputProps extends InputHTMLAttributes<HTMLInputElement> {
  label?: string;
  hint?: ReactNode;
  error?: string | null;
  icon?: ReactNode;
  trailing?: ReactNode;
}

export const TextInput = forwardRef<HTMLInputElement, TextInputProps>(function TextInput(
  { label, hint, error, icon, trailing, className, id, ...rest },
  ref,
) {
  const generatedId = useId();
  const inputId = id ?? generatedId;

  return (
    <div className="flex flex-col gap-1.5">
      {label && (
        <label htmlFor={inputId} className="text-[12.5px] font-medium text-fg-muted">
          {label}
        </label>
      )}
      <div
        className={cn(
          'flex items-center gap-2 rounded-[var(--radius-control)] bg-fill px-3',
          IS_MOBILE ? 'h-12' : 'h-9',
          // The ring is the whole of the focus treatment: a filled field has no
          // border to recolour, and the input inside draws no outline of its own.
          'ring-inset',
          error
            ? 'ring-2 ring-[var(--error)]'
            : 'focus-within:ring-2 focus-within:ring-[var(--accent)]',
          className,
        )}
      >
        {icon && <span className="shrink-0 text-fg-muted">{icon}</span>}
        <input
          ref={ref}
          id={inputId}
          aria-invalid={error ? true : undefined}
          className={cn(
            'min-w-0 flex-1 bg-transparent text-fg outline-none',
            'placeholder:text-fg-muted',
            IS_MOBILE ? 'text-[15px]' : 'text-[13.5px]',
          )}
          {...rest}
        />
        {trailing}
      </div>
      {(error || hint) && (
        <p className={cn('text-[12.5px] leading-snug', error ? 'text-error' : 'text-fg-muted')}>
          {error ?? hint}
        </p>
      )}
    </div>
  );
});

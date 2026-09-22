import { AnimatePresence, motion } from 'motion/react';
import { ClipboardPaste, CornerDownLeft, X } from 'lucide-react';
import { forwardRef, useEffect, useId, useImperativeHandle, useRef, useState } from 'react';

import { Spinner } from '@/components/ui/Spinner';
import { useTranslation } from '@/i18n';
import { cn } from '@/lib/cn';
import { T } from '@/lib/motion';
import { isProbablyUrl, normalizeUrl } from '@/lib/url';

interface UrlInputProps {
  value: string;
  onChange: (value: string) => void;
  onSubmit: (url: string) => void;
  onClear: () => void;
  onPaste: () => void;
  analyzing: boolean;
  disabled?: boolean;
  disabledHint?: string;
}

export interface UrlInputHandle {
  focus: () => void;
  select: () => void;
}

export const UrlInput = forwardRef<UrlInputHandle, UrlInputProps>(function UrlInput(
  { value, onChange, onSubmit, onClear, onPaste, analyzing, disabled = false, disabledHint },
  ref,
) {
  const { t } = useTranslation();
  const inputRef = useRef<HTMLInputElement>(null);
  const [focused, setFocused] = useState(false);
  const [touched, setTouched] = useState(false);
  const hintId = useId();

  useImperativeHandle(ref, () => ({
    focus: () => inputRef.current?.focus(),
    select: () => inputRef.current?.select(),
  }));

  // Validation only appears once the user has stopped typing something that
  // could still become a URL; complaining mid-keystroke is hostile.
  useEffect(() => {
    if (!value) setTouched(false);
  }, [value]);

  const invalid = touched && value.trim().length > 0 && !isProbablyUrl(value);
  const hint = invalid ? t('input.invalid') : disabledHint;

  const submit = () => {
    const normalized = normalizeUrl(value);
    if (!normalized) {
      setTouched(true);
      return;
    }
    onSubmit(normalized);
  };

  return (
    <div className="w-full">
      {/* The edge and the focus ring are both box-shadows, so going from one to
          the other moves nothing: at rest the card's hairline (light theme
          only), focused a 2px ring in the accent, or in red while the text is
          not an address. */}
      <div
        className={cn(
          'rounded-[var(--radius-card)] bg-surface transition-shadow duration-150 ease-out-quint',
          focused && !disabled
            ? invalid
              ? 'shadow-[0_0_0_2px_var(--error)]'
              : 'shadow-[0_0_0_2px_var(--accent)]'
            : invalid
              ? 'shadow-[inset_0_0_0_1px_var(--error)]'
              : 'shadow-[inset_0_0_0_1px_var(--card-edge)]',
        )}
      >
        <div className={cn('flex h-[52px] items-center gap-2 pl-4 pr-2', disabled && 'opacity-60')}>
          <input
            ref={inputRef}
            type="text"
            inputMode="url"
            spellCheck={false}
            autoComplete="off"
            autoCorrect="off"
            disabled={disabled}
            value={value}
            placeholder={t('input.placeholder')}
            aria-label={t('input.placeholder')}
            aria-invalid={invalid || undefined}
            aria-describedby={hint ? hintId : undefined}
            onChange={(event) => onChange(event.target.value)}
            onFocus={() => setFocused(true)}
            onBlur={() => {
              setFocused(false);
              setTouched(true);
            }}
            onKeyDown={(event) => {
              if (event.key === 'Enter') {
                event.preventDefault();
                submit();
              } else if (event.key === 'Escape' && value) {
                event.preventDefault();
                onClear();
              }
            }}
            className={cn(
              'min-w-0 flex-1 text-ellipsis bg-transparent text-[15px] text-fg outline-none',
              'placeholder:text-fg-faint',
            )}
          />

          <AnimatePresence mode="popLayout" initial={false}>
            {value.length > 0 && !analyzing && (
              <motion.button
                key="clear"
                type="button"
                initial={{ opacity: 0, scale: 0.7 }}
                animate={{ opacity: 1, scale: 1 }}
                exit={{ opacity: 0, scale: 0.7, transition: T.microOut }}
                transition={T.micro}
                // The press is animated by Motion rather than by the
                // `pressable` class, because Motion already owns this element's
                // transform for its entrance -- a CSS transition on the same
                // property would lag every frame of it.
                whileTap={{ scale: 0.9 }}
                onClick={onClear}
                aria-label={t('input.clear')}
                className="shrink-0 rounded-full p-1.5 text-fg-faint transition-colors duration-150 ease-out-quint hover:bg-fill hover:text-fg"
              >
                <X size={15} />
              </motion.button>
            )}
          </AnimatePresence>

          {value.trim().length === 0 ? (
            <button
              type="button"
              onClick={onPaste}
              disabled={disabled}
              className={cn(
                'pressable flex h-9 shrink-0 items-center gap-1.5 rounded-[var(--radius-control)] px-3',
                'text-[13px] font-medium text-fg-muted',
                'hover:bg-fill hover:text-fg disabled:pointer-events-none',
              )}
            >
              <ClipboardPaste size={15} />
              {t('input.paste')}
            </button>
          ) : (
            <button
              type="button"
              onClick={submit}
              disabled={disabled || analyzing}
              className={cn(
                'pressable flex h-9 shrink-0 items-center gap-1.5 rounded-[var(--radius-control)] px-3.5',
                'bg-accent text-[13px] font-medium text-accent-fg',
                'hover:bg-accent-hover',
                'disabled:pointer-events-none disabled:opacity-60',
              )}
            >
              {analyzing ? <Spinner size={14} /> : <CornerDownLeft size={14} />}
              {t('input.analyze')}
            </button>
          )}
        </div>
      </div>

      <AnimatePresence>
        {hint && (
          <motion.p
            id={hintId}
            initial={{ opacity: 0, y: -4 }}
            animate={{ opacity: 1, y: 0 }}
            exit={{ opacity: 0, y: -4, transition: T.microOut }}
            transition={T.micro}
            className={cn(
              'mt-2 px-1 text-[12.5px]',
              invalid ? 'text-error' : 'text-warning',
            )}
          >
            {hint}
          </motion.p>
        )}
      </AnimatePresence>
    </div>
  );
});

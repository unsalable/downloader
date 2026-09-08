import { AnimatePresence, motion } from 'motion/react';
import { ClipboardPaste, CornerDownLeft, X } from 'lucide-react';
import { forwardRef, useEffect, useImperativeHandle, useRef, useState } from 'react';

import { Spinner } from '@/components/ui/Spinner';
import { useTranslation } from '@/i18n';
import { cn } from '@/lib/cn';
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
      <div
        className={cn(
          'relative rounded-[10px] border transition-[border-color,box-shadow] duration-200',
          focused && !disabled
            ? 'border-[var(--accent)] shadow-[0_0_0_3px_var(--accent-ring)]'
            : 'border-[var(--border-strong)]',
          invalid && 'border-[var(--error)] shadow-none',
        )}
      >
        <div
          className={cn(
            'relative flex h-[54px] items-center gap-3 rounded-[9px] bg-surface pl-3.5 pr-2',
            disabled && 'opacity-60',
          )}
        >
          <span className="eyebrow shrink-0 select-none font-mono text-fg-faint">URL</span>
          <span className="h-5 w-px shrink-0 bg-[var(--border)]" />

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
              'min-w-0 flex-1 bg-transparent text-[15px] text-fg outline-none',
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
                exit={{ opacity: 0, scale: 0.7 }}
                transition={{ duration: 0.14 }}
                onClick={onClear}
                aria-label={t('input.clear')}
                className="shrink-0 rounded-md p-1 text-fg-faint transition-colors hover:bg-surface-hover hover:text-fg"
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
                'flex h-9 shrink-0 items-center gap-1.5 rounded-md px-2.5',
                'text-[12.5px] font-medium text-fg-muted transition-colors duration-150',
                'hover:bg-surface-hover hover:text-fg disabled:pointer-events-none',
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
                'flex h-9 shrink-0 items-center gap-1.5 rounded-md px-3',
                'bg-accent text-[12.5px] font-semibold text-accent-fg',
                'transition-colors duration-150 hover:bg-accent-hover active:brightness-95',
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
        {(invalid || disabledHint) && (
          <motion.p
            initial={{ opacity: 0, y: -4 }}
            animate={{ opacity: 1, y: 0 }}
            exit={{ opacity: 0, y: -4 }}
            transition={{ duration: 0.16 }}
            className={cn(
              'mt-2 px-1 text-[12.5px]',
              invalid ? 'text-error' : 'text-warning',
            )}
          >
            {invalid ? t('input.invalid') : disabledHint}
          </motion.p>
        )}
      </AnimatePresence>
    </div>
  );
});

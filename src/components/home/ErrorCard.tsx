import { motion } from 'motion/react';
import { AlertCircle, ChevronDown, Copy, RotateCw } from 'lucide-react';
import { useState } from 'react';

import { Button } from '@/components/ui/Button';
import { useTranslation } from '@/i18n';
import type { TranslationKey } from '@/i18n';
import { cn } from '@/lib/cn';
import type { AppErrorInfo } from '@/types';

interface ErrorCardProps {
  error: AppErrorInfo;
  onRetry?: () => void;
  extraAction?: { label: string; onClick: () => void };
}

/**
 * User-facing failure. The plain-language pair comes from the dictionary
 * keyed by the error code, with the backend's English text as the fallback;
 * the raw technical detail stays collapsed until asked for.
 */
export function ErrorCard({ error, onRetry, extraAction }: ErrorCardProps) {
  const { t } = useTranslation();
  const [showDetails, setShowDetails] = useState(false);
  const [copied, setCopied] = useState(false);

  const titleKey = `error.${error.code}.title` as TranslationKey;
  const messageKey = `error.${error.code}.message` as TranslationKey;
  const title = t(titleKey) === titleKey ? error.title : t(titleKey);
  const message = t(messageKey) === messageKey ? error.message : t(messageKey);

  const copyDetails = async () => {
    if (!error.technical) return;
    try {
      await navigator.clipboard.writeText(error.technical);
      setCopied(true);
      window.setTimeout(() => setCopied(false), 1600);
    } catch {
      // Clipboard permission can be refused; the text is still on screen.
    }
  };

  return (
    <motion.div
      initial={{ opacity: 0, y: 10 }}
      animate={{ opacity: 1, y: 0 }}
      exit={{ opacity: 0, y: -8 }}
      transition={{ duration: 0.3, ease: [0.22, 1, 0.36, 1] }}
      role="alert"
      className={cn(
        'overflow-hidden rounded-[var(--radius-panel)] border border-[var(--border)]',
        'bg-surface p-5 shadow-raised edge-light',
      )}
    >
      <div className="flex gap-3.5">
        <span className="mt-0.5 flex size-8 shrink-0 items-center justify-center rounded-lg bg-error-soft text-error">
          <AlertCircle size={17} />
        </span>

        <div className="min-w-0 flex-1">
          <h3 className="text-[14.5px] font-semibold text-fg">{title}</h3>
          <p className="mt-1 text-[13px] leading-relaxed text-fg-muted">{message}</p>

          <div className="mt-3.5 flex flex-wrap items-center gap-2">
            {onRetry && error.retryable && (
              <Button size="sm" variant="secondary" icon={<RotateCw size={13} />} onClick={onRetry}>
                {t('analyze.retry')}
              </Button>
            )}
            {extraAction && (
              <Button size="sm" variant="primary" onClick={extraAction.onClick}>
                {extraAction.label}
              </Button>
            )}
            {error.technical && (
              <button
                type="button"
                onClick={() => setShowDetails((value) => !value)}
                className="flex items-center gap-1 text-[12.5px] font-medium text-fg-faint transition-colors hover:text-fg-muted"
              >
                {showDetails ? t('analyze.hideDetails') : t('analyze.details')}
                <ChevronDown
                  size={13}
                  className={cn('transition-transform duration-200', showDetails && 'rotate-180')}
                />
              </button>
            )}
          </div>

          {showDetails && error.technical && (
            <motion.div
              initial={{ opacity: 0, height: 0 }}
              animate={{ opacity: 1, height: 'auto' }}
              transition={{ duration: 0.2, ease: [0.22, 1, 0.36, 1] }}
              className="overflow-hidden"
            >
              <div className="relative mt-3 rounded-lg border border-[var(--border)] bg-surface-sunken p-3">
                <pre className="selectable max-h-40 overflow-auto whitespace-pre-wrap break-words font-mono text-[11.5px] leading-relaxed text-fg-muted">
                  {error.technical}
                </pre>
                <button
                  type="button"
                  onClick={copyDetails}
                  className="absolute right-2 top-2 flex items-center gap-1 rounded-md bg-surface px-1.5 py-1 text-[11px] text-fg-faint transition-colors hover:text-fg"
                >
                  <Copy size={11} />
                  {copied ? t('analyze.copied') : t('common.copy')}
                </button>
              </div>
            </motion.div>
          )}
        </div>
      </div>
    </motion.div>
  );
}

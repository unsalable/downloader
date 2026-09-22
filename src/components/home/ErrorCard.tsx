import { motion } from 'motion/react';
import { Check, ChevronDown, CircleAlert, Copy, RotateCw } from 'lucide-react';
import { useState } from 'react';

import { Button } from '@/components/ui/Button';
import { InlineNotice } from '@/components/ui/InlineNotice';
import { useMomentary } from '@/hooks/useMomentary';
import { useTranslation } from '@/i18n';
import type { TranslationKey } from '@/i18n';
import { cn } from '@/lib/cn';
import { T, rise } from '@/lib/motion';
import type { AppErrorInfo } from '@/types';

interface ErrorCardProps {
  error: AppErrorInfo;
  onRetry?: () => void;
  extraAction?: { label: string; onClick: () => void };
  /** Why `extraAction` did not work, set beside the button that ran it. */
  actionError?: string | null;
}

/**
 * User-facing failure. The plain-language pair comes from the dictionary
 * keyed by the error code, with the backend's English text as the fallback;
 * the raw technical detail stays collapsed until asked for.
 */
export function ErrorCard({ error, onRetry, extraAction, actionError }: ErrorCardProps) {
  const { t } = useTranslation();
  const [showDetails, setShowDetails] = useState(false);
  const [copied, markCopied] = useMomentary();

  const titleKey = `error.${error.code}.title` as TranslationKey;
  const messageKey = `error.${error.code}.message` as TranslationKey;
  const title = t(titleKey) === titleKey ? error.title : t(titleKey);
  const message = t(messageKey) === messageKey ? error.message : t(messageKey);

  const copyDetails = async () => {
    if (!error.technical) return;
    try {
      await navigator.clipboard.writeText(error.technical);
      markCopied();
    } catch {
      // Clipboard permission can be refused; the text is still on screen.
    }
  };

  return (
    <motion.div
      variants={rise(10)}
      initial="initial"
      animate="animate"
      exit="exit"
      role="alert"
      className="overflow-hidden rounded-[var(--radius-card)] border border-card-edge bg-surface p-4"
    >
      <div className="flex gap-3">
        <CircleAlert size={18} aria-hidden="true" className="mt-px shrink-0 text-error" />

        <div className="min-w-0 flex-1">
          <h3 className="text-[14px] font-semibold text-fg">{title}</h3>
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
                aria-expanded={showDetails}
                className="pressable flex items-center gap-1 rounded-md px-1.5 py-1 text-[12.5px] font-medium text-fg-muted hover:text-fg"
              >
                {showDetails ? t('analyze.hideDetails') : t('analyze.details')}
                <ChevronDown
                  size={13}
                  className={cn(
                    'transition-transform duration-150 ease-out-quint',
                    showDetails && 'rotate-180',
                  )}
                />
              </button>
            )}
          </div>

          {actionError && (
            <InlineNotice tone="error" className="mt-2.5">
              {actionError}
            </InlineNotice>
          )}

          {showDetails && error.technical && (
            <motion.div
              initial={{ opacity: 0, height: 0 }}
              animate={{ opacity: 1, height: 'auto' }}
              transition={T.component}
              className="overflow-hidden"
            >
              {/* The one place on Home set in mono: raw engine output, which
                  is copied into a bug report character for character. */}
              <div className="mt-3 rounded-[var(--radius-control)] bg-fill p-3">
                <pre className="selectable max-h-40 overflow-auto whitespace-pre-wrap break-words font-mono text-[12px] leading-relaxed text-fg-muted">
                  {error.technical}
                </pre>
                <button
                  type="button"
                  onClick={copyDetails}
                  className="pressable -ml-1.5 mt-2 flex items-center gap-1 rounded-md px-1.5 py-1 text-[12.5px] font-medium text-fg-muted hover:text-fg"
                >
                  {copied ? <Check size={13} /> : <Copy size={13} />}
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

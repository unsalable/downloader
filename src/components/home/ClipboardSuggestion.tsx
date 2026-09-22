import { Clipboard, X } from 'lucide-react';

import { useTranslation } from '@/i18n';
import { cn } from '@/lib/cn';
import { IS_MOBILE } from '@/lib/platform';
import { displayHost } from '@/lib/url';

interface ClipboardSuggestionProps {
  url: string;
  onAccept: () => void;
  onDismiss: () => void;
}

/**
 * A link that was on the clipboard when the window came back, offered rather
 * than acted on. One line, the height of the platform row it stands in for, so
 * nothing on the page moves when it turns up.
 */
export function ClipboardSuggestion({ url, onAccept, onDismiss }: ClipboardSuggestionProps) {
  const { t } = useTranslation();

  return (
    <div
      role="group"
      aria-label={t('input.suggestion')}
      className="flex h-8 items-center justify-center gap-1 text-[12.5px]"
    >
      <Clipboard size={14} aria-hidden="true" className="shrink-0 text-fg-faint" />
      {/* The host says which link this is; the rest of an address is noise. */}
      <span className="ml-1 min-w-0 max-w-[220px] truncate text-fg-muted">
        {displayHost(url) ?? url}
      </span>

      <button
        type="button"
        onClick={onAccept}
        className={cn(
          'pressable shrink-0 rounded-[8px] font-medium text-accent hover:bg-accent-soft',
          IS_MOBILE ? 'h-8 px-3' : 'h-7 px-2',
        )}
      >
        {t('input.analyze')}
      </button>

      <button
        type="button"
        onClick={onDismiss}
        aria-label={t('input.suggestionDismiss')}
        className={cn(
          'pressable-sm flex shrink-0 items-center justify-center rounded-full text-fg-faint',
          'hover:bg-fill hover:text-fg',
          IS_MOBILE ? 'size-8' : 'size-7',
        )}
      >
        <X size={14} />
      </button>
    </div>
  );
}

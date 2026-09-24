import { writeText } from '@tauri-apps/plugin-clipboard-manager';
import { Check, Copy } from 'lucide-react';
import { useState } from 'react';

import { useMomentary } from '@/hooks/useMomentary';
import { useTranslation } from '@/i18n';
import { cn } from '@/lib/cn';
import { displayLink } from '@/lib/url';

/**
 * Where a download came from, as the last line of its row: the link, cut down
 * to what can be read at a glance, and pressing it puts the whole link on the
 * clipboard. Nothing pops up to say so -- the icon after it turns into a check
 * for a moment, and a failure takes the link's place for as long.
 *
 * It sits in a row that opens its file when pressed. That row's button lies
 * over the row rather than around it (a button may not hold another), and this
 * one is lifted above it, so a press here copies and opens nothing.
 */
export function SourceLink({ url, className }: { url: string; className?: string }) {
  const { t } = useTranslation();
  // One moment for either answer, so the newest press is the one on screen.
  const [showing, show] = useMomentary(2000);
  const [outcome, setOutcome] = useState<'copied' | 'failed'>('copied');
  const copied = showing && outcome === 'copied';
  const failed = showing && outcome === 'failed';

  const copy = async () => {
    try {
      await writeText(url);
      setOutcome('copied');
    } catch {
      // Another program can be holding the clipboard.
      setOutcome('failed');
    }
    show();
  };

  return (
    <div className={cn('flex min-w-0', className)}>
      <button
        type="button"
        onClick={() => void copy()}
        title={url}
        className={cn(
          'relative z-[1] flex min-w-0 max-w-full items-center gap-1.5 rounded-[6px] text-left',
          // Pressable below its line as well, into the row's own padding, so a
          // fingertip has more than 18px to find. Not above it: that is the
          // line before, and on a failed row, its Details button.
          '-mb-2 pb-2 text-[12.5px] leading-[18px]',
          'text-fg-muted transition-colors duration-150 ease-out-quint hover:text-fg',
        )}
      >
        <span className="sr-only">{t('downloads.copyLink')}: </span>
        <span className={cn('min-w-0 truncate', failed && 'text-error')}>
          {failed ? t('downloads.copyFailed') : displayLink(url)}
        </span>
        {copied ? (
          <Check size={13} aria-hidden="true" className="shrink-0 text-success" />
        ) : (
          <Copy size={13} aria-hidden="true" className="shrink-0" />
        )}
      </button>
      <span role="status" className="sr-only">
        {copied ? t('downloads.linkCopied') : failed ? t('downloads.copyFailed') : ''}
      </span>
    </div>
  );
}

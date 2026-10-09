import { Check } from 'lucide-react';
import { useEffect, useState, type KeyboardEvent } from 'react';

import { Button } from '@/components/ui/Button';
import { InlineNotice } from '@/components/ui/InlineNotice';
import { ListGroup } from '@/components/ui/ListGroup';
import { Modal } from '@/components/ui/Modal';
import { useTranslation } from '@/i18n';
import { cn } from '@/lib/cn';
import { IS_MOBILE } from '@/lib/platform';
import * as ipc from '@/services/ipc';
import type { HistoryRange } from '@/types';
import { CLEAR_RANGES, DEFAULT_CLEAR_RANGE, RANGE_LABEL, clearSummaryKey } from './clearRange';

type Counts = Record<HistoryRange, number>;

interface ClearHistoryModalProps {
  open: boolean;
  onClose: () => void;
  /** The chosen range is gone; the caller reads its list again and closes. */
  onCleared: () => void;
}

/**
 * Clearing the history, Safari's way: how far back, how much that is, and a
 * Clear that takes only that. The files themselves stay where they are.
 *
 * The range reaches across the whole history, whatever the search above the
 * list is showing, as clearing all of it always did; the sentence under the
 * choices counts the range itself, so it says so.
 */
export function ClearHistoryModal({ open, onClose, onCleared }: ClearHistoryModalProps) {
  const { t } = useTranslation();
  const [range, setRange] = useState<HistoryRange>(DEFAULT_CLEAR_RANGE);
  // null while reading; 'unknown' when the counts could not be read.
  const [counts, setCounts] = useState<Counts | 'unknown' | null>(null);
  const [clearing, setClearing] = useState(false);
  const [failed, setFailed] = useState(false);

  // The dialog keeps nothing between openings (as LinkImportModal does): it is
  // put back when it closes, so the next opening starts at the narrowest range
  // with fresh numbers. The panel that is fading out keeps its last frame, so
  // the reset is never seen. All three counts are read at once, so moving
  // between the rows is instant and the sentence never flickers.
  useEffect(() => {
    if (!open) {
      setRange(DEFAULT_CLEAR_RANGE);
      setCounts(null);
      setFailed(false);
      return;
    }
    let live = true;
    // An explicit tuple: mapped over CLEAR_RANGES, the results would type as
    // `number | undefined` under noUncheckedIndexedAccess.
    Promise.all([ipc.countHistory('day'), ipc.countHistory('week'), ipc.countHistory('all')])
      .then(([day, week, all]) => {
        if (live) setCounts({ day, week, all });
      })
      // Unreadable counts leave the sentence out, not the button: clearing can
      // still work, and a dialog that cannot be used is worse than one that
      // cannot say how much.
      .catch(() => {
        if (live) setCounts('unknown');
      });
    return () => {
      live = false;
    };
  }, [open]);

  const count = counts === null || counts === 'unknown' ? null : counts[range];
  const canClear = counts !== null && count !== 0 && !clearing;

  const clear = async () => {
    if (!canClear) return;
    setClearing(true);
    setFailed(false);
    try {
      await ipc.clearHistory(range);
      onCleared();
    } catch {
      // Said here, under the choices, in words of its own: the backend's
      // generic failure is about a download, which this is not.
      setFailed(true);
    } finally {
      setClearing(false);
    }
  };

  // The arrows move the choice and the focus together, as they do between a
  // set of radio buttons anywhere else.
  const step = (event: KeyboardEvent<HTMLDivElement>) => {
    const by = event.key === 'ArrowDown' ? 1 : event.key === 'ArrowUp' ? -1 : 0;
    if (by === 0) return;
    event.preventDefault();
    if (clearing) return;
    const index = (CLEAR_RANGES.indexOf(range) + by + CLEAR_RANGES.length) % CLEAR_RANGES.length;
    setRange(CLEAR_RANGES[index]!);
    event.currentTarget.querySelectorAll<HTMLElement>('[role="radio"]')[index]?.focus();
  };

  return (
    <Modal
      open={open}
      onClose={onClose}
      title={t('history.clearAll')}
      description={t('history.clearConfirmBody')}
      closeLabel={t('common.close')}
      footer={
        <>
          <Button variant="ghost" onClick={onClose}>
            {t('common.cancel')}
          </Button>
          <Button variant="danger" loading={clearing} disabled={!canClear} onClick={() => void clear()}>
            {t('history.clear')}
          </Button>
        </>
      }
    >
      <div className="pb-3">
        {/* The radiogroup wraps the group rather than sitting inside it: the
            hairlines are drawn between the group's direct children. */}
        <div role="radiogroup" aria-label={t('history.clearRange')} onKeyDown={step}>
          <ListGroup tone="fill">
            {CLEAR_RANGES.map((value) => {
              const selected = value === range;
              return (
                <button
                  key={value}
                  type="button"
                  role="radio"
                  aria-checked={selected}
                  // Enter on opening picks a range; it never clears anything.
                  data-autofocus={selected ? '' : undefined}
                  // Not `disabled` while clearing: the row holding the focus
                  // would drop it to the page behind the dialog, and Tab would
                  // follow it there.
                  onClick={() => {
                    if (!clearing) setRange(value);
                  }}
                  className={cn(
                    'flex w-full items-center gap-3 px-4 text-left transition-colors duration-150 ease-out-quint',
                    'hover:bg-fill active:bg-fill-hover',
                    IS_MOBILE ? 'min-h-12 py-2.5 text-[15px]' : 'min-h-10 py-2 text-[13.5px]',
                  )}
                >
                  <span className="min-w-0 flex-1 text-fg">{t(RANGE_LABEL[value])}</span>
                  {selected && (
                    <Check
                      size={IS_MOBILE ? 18 : 16}
                      strokeWidth={2.25}
                      aria-hidden="true"
                      className="shrink-0 text-accent"
                    />
                  )}
                </button>
              );
            })}
          </ListGroup>
        </div>
        {/* Reserved height. On a phone every sentence with a number takes two
            lines and the "none" ones take one, and the centred dialog would bob
            by half a line each time the choice changed. */}
        <p
          role="status"
          className={cn(
            'mt-2 px-4 leading-snug text-fg-muted',
            IS_MOBILE ? 'min-h-[2.75em] text-[13px]' : 'min-h-[1.375em] text-[12.5px]',
          )}
        >
          {count == null ? '' : t(clearSummaryKey(range, count), { n: count })}
        </p>
        {failed && (
          <InlineNotice tone="error" className="mt-1 px-4">
            {t('history.clearFailed')}
          </InlineNotice>
        )}
      </div>
    </Modal>
  );
}

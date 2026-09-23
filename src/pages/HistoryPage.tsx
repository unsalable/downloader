import { HistoryList } from '@/components/history/HistoryList';

/**
 * The desktop's History screen. A phone has none of its own: there the history
 * is the second half of Downloads, and the app sends anything bound for this
 * screen to that half instead.
 */
export function HistoryPage({ onGoHome }: { onGoHome: () => void }) {
  return (
    <div className="mx-auto w-full max-w-[760px] px-6 pb-12">
      <HistoryList onGoHome={onGoHome} />
    </div>
  );
}

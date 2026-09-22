import { AnimatePresence, motion } from 'motion/react';
import { useEffect } from 'react';

import { Spinner } from '@/components/ui/Spinner';
import { useTranslation } from '@/i18n';
import { FADE } from '@/lib/motion';
import { IS_MOBILE } from '@/lib/platform';
import { useAnalysisStore } from '@/stores/useAnalysisStore';
import { selectConvertInFlight, useConvertStore } from '@/stores/useConvertStore';
import { selectInFlightCount, useQueueStore } from '@/stores/useQueueStore';
import { useToolsStore } from '@/stores/useToolsStore';
import { useUpdateStore } from '@/stores/useUpdateStore';

/**
 * The desktop app can sit in the tray for days, so a launch is not the only
 * time it looks: coming back to the window this long after the last check
 * counts as one too.
 */
const RECHECK_AFTER_MS = 6 * 60 * 60 * 1000;

/** Long enough to read why the window is about to close, and no longer. */
const NOTICE_MS = 1200;

async function checkAndStage() {
  const store = useUpdateStore.getState();
  const found = await store.check();
  // `stalled` is decided by the check, so it is read again afterwards.
  if (found && !useUpdateStore.getState().stalled) await store.install();
}

/**
 * Keeps the desktop app current without asking.
 *
 * A newer build is downloaded in the background with nothing on screen, and
 * installed at the first moment that interrupts nobody: no download or
 * conversion in flight, no link half way to becoming one. Installing closes
 * the app, so the window says so for a moment first. Every failure on the way
 * is silent -- offline is a normal state -- and shows only on the About page.
 */
export function DesktopUpdater() {
  const { t } = useTranslation();
  const phase = useUpdateStore((state) => state.phase);
  const stalled = useUpdateStore((state) => state.stalled);
  // A list that has not loaded yet is not an empty one: the queue restores
  // itself at launch, which is also when a staged update is first looked at.
  const queueQuiet = useQueueStore(
    (state) => state.loaded && selectInFlightCount(state.tasks) === 0,
  );
  // A picked batch and the format chosen for it are minutes of assembly that
  // live only in memory, so a list waiting for Convert counts as busy too.
  const convertQuiet = useConvertStore(
    (state) =>
      state.loaded &&
      selectConvertInFlight(state.jobs) === 0 &&
      state.files.length === 0 &&
      !state.submitting,
  );
  // The same on Home: a link in the field, or an error card waiting for Retry.
  const homeQuiet = useAnalysisStore((state) => state.phase === 'idle' && state.url.trim() === '');
  // Installing an engine is a long download running inside this process, with
  // its own progress bar on screen; exiting mid-transfer throws away a press
  // the user is watching. The selector returns the boolean, not the map, so the
  // progress ticks behind it do not re-render the overlay.
  const toolsQuiet = useToolsStore((state) => Object.keys(state.installing).length === 0);

  useEffect(() => {
    // The phone asks first (see `UpdatePrompt`); installing unasked there
    // would throw the system installer at the user out of nowhere.
    if (IS_MOBILE) return;
    void checkAndStage();

    const onReturn = () => {
      if (document.visibilityState !== 'visible') return;
      if (Date.now() - useUpdateStore.getState().lastCheckedAt >= RECHECK_AFTER_MS) {
        void checkAndStage();
      }
    };
    document.addEventListener('visibilitychange', onReturn);
    window.addEventListener('focus', onReturn);
    return () => {
      document.removeEventListener('visibilitychange', onReturn);
      window.removeEventListener('focus', onReturn);
    };
  }, []);

  // Work in flight on any of these screens is deferred, not lost: the next
  // launch starts with an empty store and installs then.
  const idle = queueQuiet && convertQuiet && homeQuiet && toolsQuiet;
  const due = phase === 'ready' && !stalled && idle;

  // Work that starts while the notice is up takes the turn back.
  useEffect(() => {
    if (!due) return;
    // A Settings field writes its text when it loses focus, and the overlay
    // covers it without taking focus away, so the caret is sent off here --
    // which leaves the whole notice for that write to land before the exit.
    (document.activeElement as HTMLElement | null)?.blur();
    const timer = window.setTimeout(() => void useUpdateStore.getState().apply(), NOTICE_MS);
    return () => window.clearTimeout(timer);
  }, [due]);

  return (
    <AnimatePresence>
      {(due || phase === 'applying') && (
        <motion.div
          variants={FADE}
          initial="initial"
          animate="animate"
          exit="exit"
          // Nothing here can be answered, so it is announced rather than
          // presented as a dialog to get out of.
          role="status"
          className="fixed inset-0 z-[950] flex flex-col items-center justify-center bg-bg px-8 text-center"
        >
          <Spinner size={22} className="text-fg-muted" />
          <h2 className="mt-5 text-[17px] font-semibold tracking-[-0.01em] text-fg">
            {t('update.installing')}
          </h2>
          <p className="mt-1 text-[13.5px] text-fg-muted">{t('update.restarting')}</p>
        </motion.div>
      )}
    </AnimatePresence>
  );
}

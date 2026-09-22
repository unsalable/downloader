import { useEffect, useState } from 'react';

import { Button } from '@/components/ui/Button';
import { InlineNotice } from '@/components/ui/InlineNotice';
import { SettingRow } from '@/components/ui/SettingRow';
import { useTranslation } from '@/i18n';
import type { TranslationKey } from '@/i18n';
import { IS_MOBILE } from '@/lib/platform';
import * as ipc from '@/services/ipc';
import { useUpdateStore } from '@/stores/useUpdateStore';

/**
 * Which build this is, how it stands against the released one, and a way to
 * ask now rather than at the next launch. A row for a `ListGroup`, like any
 * `SettingRow`.
 *
 * The app's own update attempts fail without a word -- being offline is normal
 * -- and leave at most a changed status here. Only a check the user started
 * from this row answers with an error.
 */
export function UpdateRow() {
  const { t } = useTranslation();
  const update = useUpdateStore((state) => state.update);
  const phase = useUpdateStore((state) => state.phase);
  const checked = useUpdateStore((state) => state.checked);
  const stalled = useUpdateStore((state) => state.stalled);
  // Null until the backend has answered, so the row does not name a build it
  // has not been told about yet.
  const [commit, setCommit] = useState<string | null>(null);
  const [checking, setChecking] = useState(false);
  const [problem, setProblem] = useState<TranslationKey | null>(null);

  useEffect(() => {
    void ipc
      .getBuildCommit()
      .then(setCommit)
      .catch(() => setCommit(''));
  }, []);

  const run = async () => {
    setProblem(null);
    setChecking(true);
    try {
      const { check, install } = useUpdateStore.getState();
      const found = await check(true);
      // On the phone the prompt has opened by now. The desktop has none: the
      // build is staged here, and `DesktopUpdater` installs it as soon as
      // nothing is running.
      if (found && !IS_MOBILE) {
        await install();
        if (useUpdateStore.getState().phase === 'failed') setProblem('update.failed');
      }
    } catch {
      setProblem('update.checkFailed');
    } finally {
      setChecking(false);
    }
  };

  const status = ((): TranslationKey | null => {
    if (phase === 'downloading') return 'update.downloading';
    if (phase === 'failed' || stalled) return 'update.notInstalled';
    if (update) return !IS_MOBILE && phase === 'ready' ? 'update.waiting' : 'update.title';
    return checked ? 'update.current' : null;
  })();

  // A build made outside a git checkout has no commit to be named by.
  const named = commit ? t('update.build', { commit: commit.slice(0, 7) }) : t('update.buildUnknown');
  const build = commit === null ? null : named;

  return (
    <SettingRow
      // Holds its line while the backend is still being asked.
      title={<span className="block min-h-[1lh] tabular-nums">{build}</span>}
      description={status && t(status)}
      control={
        <Button
          size="sm"
          variant="secondary"
          loading={checking}
          disabled={phase === 'downloading' || phase === 'applying'}
          onClick={() => void run()}
        >
          {t('update.check')}
        </Button>
      }
    >
      {problem && (
        <InlineNotice tone="error" className="-mt-2 w-full">
          {t(problem)}
        </InlineNotice>
      )}
    </SettingRow>
  );
}

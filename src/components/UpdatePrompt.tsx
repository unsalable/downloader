import { Download, RotateCw, Sparkles } from 'lucide-react';
import { useEffect } from 'react';

import { Button } from '@/components/ui/Button';
import { Modal } from '@/components/ui/Modal';
import { Progress } from '@/components/ui/Progress';
import { useTranslation } from '@/i18n';
import { formatBytes, formatDate } from '@/lib/format';
import * as ipc from '@/services/ipc';
import { useUpdateStore } from '@/stores/useUpdateStore';

/**
 * Returning to the app counts as opening it, but a phone switches between apps
 * constantly; a check this long after the last one is plenty.
 */
const RECHECK_AFTER_MS = 30 * 60 * 1000;

/**
 * Looks for a newer build each time the phone app is opened, and offers it.
 *
 * Nothing is shown while the check runs or when it finds nothing -- the app
 * simply carries on -- so the only visible trace of a check is an update.
 */
export function UpdatePrompt() {
  const { t, language } = useTranslation();
  const update = useUpdateStore((state) => state.update);
  const open = useUpdateStore((state) => state.open);
  const phase = useUpdateStore((state) => state.phase);
  const progress = useUpdateStore((state) => state.progress);
  const error = useUpdateStore((state) => state.error);
  const install = useUpdateStore((state) => state.install);
  const postpone = useUpdateStore((state) => state.postpone);

  useEffect(() => {
    const { check } = useUpdateStore.getState();
    void check();

    const onVisible = () => {
      if (document.visibilityState !== 'visible') return;
      const state = useUpdateStore.getState();
      if (Date.now() - state.lastCheckedAt >= RECHECK_AFTER_MS) void state.check();
    };
    document.addEventListener('visibilitychange', onVisible);

    const unlisten = ipc.onUpdateProgress((value) => useUpdateStore.getState().setProgress(value));
    return () => {
      document.removeEventListener('visibilitychange', onVisible);
      void unlisten.then((off) => off());
    };
  }, []);

  if (!update) return null;

  const busy = phase === 'downloading';
  const total = progress?.totalBytes ?? update.assetSize;
  const received = progress?.receivedBytes ?? 0;
  const fetched = busy && progress != null && received >= total;
  const percent = total > 0 ? (received / total) * 100 : null;
  const published = update.publishedAt ? Date.parse(update.publishedAt) : NaN;

  const body = (() => {
    if (phase === 'failed' && error) {
      return error.code === 'permission' ? t('update.permission') : t('update.failed');
    }
    if (phase === 'ready') return t('update.installerOpened');
    return t('update.body');
  })();

  return (
    <Modal
      open={open}
      // The download cannot be abandoned half way from here, so the prompt
      // stays until it has handed over to the installer.
      onClose={busy ? () => {} : postpone}
      title={
        <span className="flex items-center gap-2">
          <Sparkles size={17} className="shrink-0 text-accent" />
          {t('update.title')}
        </span>
      }
      description={body}
      closeLabel={t('common.close')}
      footer={
        <>
          <Button variant="ghost" onClick={postpone} disabled={busy}>
            {phase === 'ready' ? t('common.close') : t('update.later')}
          </Button>
          <Button
            variant="primary"
            data-autofocus
            loading={busy}
            icon={phase === 'failed' ? <RotateCw size={15} /> : <Download size={15} />}
            onClick={() => void install()}
          >
            {phase === 'failed' ? t('update.retry') : t('update.install')}
          </Button>
        </>
      }
    >
      <div className="pb-3">
        {busy ? (
          <div className="rounded-[var(--radius-card)] border border-[var(--border)] bg-surface-sunken p-3">
            <div className="metric mb-2 flex items-center justify-between text-[12px] text-fg-muted">
              <span>{fetched ? t('update.preparing') : t('update.downloading')}</span>
              <span>
                {formatBytes(received)} / {formatBytes(total)}
              </span>
            </div>
            <Progress value={fetched ? null : percent} label={t('update.downloading')} />
          </div>
        ) : (
          <div className="metric flex flex-wrap gap-x-4 gap-y-1 text-[12px] text-fg-faint">
            <span>
              {t('update.size')}: {formatBytes(update.assetSize)}
            </span>
            {Number.isFinite(published) && <span>{formatDate(published, language)}</span>}
          </div>
        )}
      </div>
    </Modal>
  );
}

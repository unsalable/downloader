import { openUrl } from '@tauri-apps/plugin-opener';
import { ExternalLink, Play, ShieldCheck } from 'lucide-react';
import { useEffect, useState } from 'react';

import { UpdateRow } from '@/components/settings/UpdateRow';
import { ListGroup } from '@/components/ui/ListGroup';
import { Logo } from '@/components/layout/Logo';
import { PageHeader } from '@/components/ui/PageHeader';
import { SettingGroup, SettingRow } from '@/components/ui/SettingRow';
import { useTranslation } from '@/i18n';
import { cn } from '@/lib/cn';
import { IS_MOBILE } from '@/lib/platform';
import * as ipc from '@/services/ipc';
import type { DiagnosticsSnapshot, LicenseEntry } from '@/types';

// The sizes a `SettingRow` sets its two lines in, so every row on the page agrees.
const TITLE_SIZE = IS_MOBILE ? 'text-[15px]' : 'text-[13.5px]';
const BODY_SIZE = IS_MOBILE ? 'text-[13px]' : 'text-[12.5px]';

/**
 * `onPlayIntro` is the phone's: the first-run film, played again on request.
 * Without it -- the desktop, which has no film -- the row is not there.
 */
export function AboutPage({ onPlayIntro }: { onPlayIntro?: () => void }) {
  const { t } = useTranslation();
  const [licenses, setLicenses] = useState<LicenseEntry[]>([]);
  const [diagnostics, setDiagnostics] = useState<DiagnosticsSnapshot | null>(null);

  useEffect(() => {
    void ipc
      .getLicenses()
      .then((payload) => setLicenses(payload.packages))
      .catch(() => setLicenses([]));
    void ipc.getDiagnostics().then(setDiagnostics).catch(() => setDiagnostics(null));
  }, []);

  const tools = licenses.filter((entry) => entry.kind === 'external-tool');
  const libraries = licenses.filter((entry) => entry.kind !== 'external-tool');

  return (
    <div className={cn('mx-auto w-full max-w-[720px] pb-12', IS_MOBILE ? 'px-4 pt-4' : 'px-6')}>
      <PageHeader title={t('nav.about')} />

      <div className="flex flex-col gap-6">
        {/* The app itself: what it is, then which build and whether a newer one
            exists. Releases carry no version number; the build's commit is what
            tells one from the next. */}
        <ListGroup>
          <div className={cn('flex items-center gap-3.5 px-4', IS_MOBILE ? 'py-4' : 'py-3.5')}>
            <Logo size={40} className="shrink-0" />
            {/* English whatever the interface language, and marked as such so
                it is read out -- and cased -- as English. */}
            <div lang="en" className="min-w-0 truncate text-[15px] font-semibold text-fg">
              {t('app.name')}
            </div>
          </div>
          <UpdateRow />
          {onPlayIntro && (
            <button
              type="button"
              onClick={onPlayIntro}
              className={cn(
                'flex w-full items-center gap-3 px-4 text-left',
                'transition-colors duration-150 ease-out-quint active:bg-surface-active',
                IS_MOBILE ? 'py-3.5' : 'py-2.5',
              )}
            >
              <span className={cn('min-w-0 flex-1 text-fg', TITLE_SIZE)}>{t('intro.replay')}</span>
              <Play size={15} aria-hidden="true" className="shrink-0 text-fg-faint" />
            </button>
          )}
        </ListGroup>

        <ListGroup>
          <SettingRow
            title={
              <span className="flex items-center gap-1.5">
                <ShieldCheck size={15} aria-hidden="true" className="shrink-0 text-success" />
                {t('about.privacyTitle')}
              </span>
            }
            description={t(IS_MOBILE ? 'about.privacyBodyMobile' : 'about.privacyBody')}
          />
        </ListGroup>

        {diagnostics && (
          <SettingGroup title={t('about.system')}>
            <DiagnosticRow label="OS" value={diagnostics.os} />
            <DiagnosticRow
              label={t('settings.engine')}
              value={
                diagnostics.engine.available
                  ? (diagnostics.engine.version ?? t('common.enabled'))
                  : t('settings.toolMissing')
              }
            />
            <DiagnosticRow
              label={t('settings.ffmpeg')}
              value={
                diagnostics.ffmpeg.available
                  ? (diagnostics.ffmpeg.version ?? t('common.enabled'))
                  : t('settings.toolMissing')
              }
            />
            <DiagnosticRow label={t('settings.downloadDir')} value={diagnostics.downloadDir} />
          </SettingGroup>
        )}

        {tools.length > 0 && (
          <SettingGroup
            title={t('about.externalTools')}
            description={t(IS_MOBILE ? 'setup.bodyMobile' : 'setup.body')}
          >
            {tools.map((entry) => (
              <LicenseRow key={entry.name} entry={entry} />
            ))}
          </SettingGroup>
        )}

        {libraries.length > 0 && (
          <SettingGroup title={t('about.licenses')}>
            {libraries.map((entry) => (
              <LicenseRow key={`${entry.kind}-${entry.name}`} entry={entry} />
            ))}
          </SettingGroup>
        )}
      </div>
    </div>
  );
}

function DiagnosticRow({ label, value }: { label: string; value: string }) {
  return (
    <div
      className={cn(
        'flex items-baseline justify-between gap-6 px-4',
        IS_MOBILE ? 'py-3.5' : 'py-2.5',
      )}
    >
      <span className={cn('shrink-0 text-fg', TITLE_SIZE)}>{label}</span>
      {/* A long path breaks where it has to rather than being cut short: this
          is the text someone copies into a bug report. */}
      <span
        className={cn(
          'selectable tabular min-w-0 text-right text-fg-muted [overflow-wrap:anywhere]',
          BODY_SIZE,
        )}
      >
        {value}
      </span>
    </div>
  );
}

function LicenseRow({ entry }: { entry: LicenseEntry }) {
  return (
    <button
      type="button"
      // Opens in the user's own browser rather than a webview: an external
      // page has no business running inside the app's origin.
      onClick={() => void openUrl(entry.url)}
      className={cn(
        'flex w-full items-center gap-3 px-4 text-left',
        'transition-colors duration-150 ease-out-quint hover:bg-surface-hover active:bg-surface-active',
        IS_MOBILE ? 'py-3.5' : 'py-2.5',
      )}
    >
      <span className="min-w-0 flex-1">
        <span className="flex items-baseline gap-2">
          <span className={cn('truncate text-fg', TITLE_SIZE)}>{entry.name}</span>
          {entry.version && entry.version !== 'variable' && (
            <span className={cn('tabular shrink-0 text-fg-faint', BODY_SIZE)}>{entry.version}</span>
          )}
        </span>
        {entry.note && (
          <span className={cn('mt-0.5 block leading-relaxed text-fg-muted', BODY_SIZE)}>
            {entry.note}
          </span>
        )}
      </span>

      <span className={cn('max-w-[42%] shrink-0 text-right text-fg-muted', BODY_SIZE)}>
        {entry.license}
      </span>
      <ExternalLink size={14} aria-hidden="true" className="shrink-0 text-fg-faint" />
    </button>
  );
}

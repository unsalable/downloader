import { motion } from 'motion/react';
import { openUrl } from '@tauri-apps/plugin-opener';
import { ExternalLink, ShieldCheck } from 'lucide-react';
import { useEffect, useState } from 'react';

import { Badge } from '@/components/ui/Badge';
import { Logo } from '@/components/layout/Logo';
import { SettingGroup } from '@/components/ui/SettingRow';
import { useTranslation } from '@/i18n';
import { cn } from '@/lib/cn';
import * as ipc from '@/services/ipc';
import type { DiagnosticsSnapshot, LicenseEntry } from '@/types';

const KIND_LABEL: Record<LicenseEntry['kind'], string> = {
  npm: 'npm',
  cargo: 'crate',
  font: 'font',
  'external-tool': 'tool',
};

export function AboutPage() {
  const { t } = useTranslation();
  const [version, setVersion] = useState('');
  const [licenses, setLicenses] = useState<LicenseEntry[]>([]);
  const [diagnostics, setDiagnostics] = useState<DiagnosticsSnapshot | null>(null);

  useEffect(() => {
    void ipc.getAppVersion().then(setVersion);
    void ipc
      .getLicenses()
      .then((payload) => setLicenses(payload.packages))
      .catch(() => setLicenses([]));
    void ipc.getDiagnostics().then(setDiagnostics).catch(() => setDiagnostics(null));
  }, []);

  const tools = licenses.filter((entry) => entry.kind === 'external-tool');
  const libraries = licenses.filter((entry) => entry.kind !== 'external-tool');

  return (
    <div className="mx-auto w-full max-w-[720px] px-6 pb-12">
      <motion.header
        initial={{ opacity: 0, y: 10 }}
        animate={{ opacity: 1, y: 0 }}
        transition={{ duration: 0.35, ease: [0.22, 1, 0.36, 1] }}
        className="flex flex-col items-center py-8 text-center"
      >
        <Logo size={52} />
        <h2 className="mt-4 text-[19px] font-semibold tracking-[-0.02em] text-fg">
          {t('app.name')}
        </h2>
        <p className="mt-1 text-[13px] text-fg-muted">
          {version ? t('about.version', { version }) : ' '}
        </p>
        <p className="mt-2 max-w-[380px] text-[13px] leading-relaxed text-fg-muted">
          {t('about.description')}
        </p>
      </motion.header>

      <section
        className={cn(
          'rounded-[var(--radius-card)] border border-[var(--success)]/25',
          'bg-[linear-gradient(135deg,var(--success-soft),transparent_60%)] p-4',
        )}
      >
        <div className="flex gap-3">
          <ShieldCheck size={18} className="mt-0.5 shrink-0 text-success" />
          <div>
            <h3 className="text-[13.5px] font-semibold text-fg">{t('about.privacyTitle')}</h3>
            <p className="mt-1 text-[12.5px] leading-relaxed text-fg-muted">
              {t('about.privacyBody')}
            </p>
          </div>
        </div>
      </section>

      {diagnostics && (
        <div className="mt-6">
          <SettingGroup title={t('settings.advanced')}>
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
        </div>
      )}

      {tools.length > 0 && (
        <div className="mt-6">
          <SettingGroup
            title={t('about.externalTools')}
            description={t('setup.body')}
          >
            {tools.map((entry) => (
              <LicenseRow key={entry.name} entry={entry} />
            ))}
          </SettingGroup>
        </div>
      )}

      {libraries.length > 0 && (
        <div className="mt-6">
          <SettingGroup title={t('about.licenses')}>
            {libraries.map((entry) => (
              <LicenseRow key={`${entry.kind}-${entry.name}`} entry={entry} />
            ))}
          </SettingGroup>
        </div>
      )}
    </div>
  );
}

function DiagnosticRow({ label, value }: { label: string; value: string }) {
  return (
    <div className="flex items-center justify-between gap-4 px-4 py-2.5">
      <span className="text-[12.5px] text-fg-muted">{label}</span>
      <span className="selectable truncate font-mono text-[11.5px] text-fg">{value}</span>
    </div>
  );
}

function LicenseRow({ entry }: { entry: LicenseEntry }) {
  return (
    <div className="flex items-center gap-3 px-4 py-2.5">
      <div className="min-w-0 flex-1">
        <div className="flex items-center gap-2">
          <span className="truncate text-[13px] font-medium text-fg">{entry.name}</span>
          {entry.version && entry.version !== 'variable' && (
            <span className="tabular shrink-0 text-[11.5px] text-fg-faint">{entry.version}</span>
          )}
          <Badge tone="outline">{KIND_LABEL[entry.kind]}</Badge>
        </div>
        {entry.note && (
          <p className="mt-0.5 text-[11.5px] leading-relaxed text-fg-faint">{entry.note}</p>
        )}
      </div>

      <span className="shrink-0 text-[11.5px] text-fg-muted">{entry.license}</span>

      <button
        type="button"
        // Opens in the user's own browser rather than a webview: an external
        // page has no business running inside the app's origin.
        onClick={() => void openUrl(entry.url)}
        aria-label={entry.url}
        className="shrink-0 rounded-md p-1 text-fg-faint transition-colors hover:bg-surface-hover hover:text-fg"
      >
        <ExternalLink size={13} />
      </button>
    </div>
  );
}

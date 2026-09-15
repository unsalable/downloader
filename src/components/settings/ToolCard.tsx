import { open } from '@tauri-apps/plugin-dialog';
import { CheckCircle2, CircleAlert, Download, FolderSearch, RotateCw } from 'lucide-react';

import { Badge } from '@/components/ui/Badge';
import { Button } from '@/components/ui/Button';
import { Progress } from '@/components/ui/Progress';
import { Tooltip } from '@/components/ui/Tooltip';
import { useTranslation } from '@/i18n';
import type { TranslationKey } from '@/i18n';
import { cn } from '@/lib/cn';
import { formatBytes, truncateMiddle } from '@/lib/format';
import { IS_MOBILE } from '@/lib/platform';
import type { ToolInstallProgress, ToolStatus } from '@/types';

interface ToolCardProps {
  status: ToolStatus;
  titleKey: TranslationKey;
  hintKey: TranslationKey;
  installing: ToolInstallProgress | undefined;
  customPath: string | null;
  onInstall: () => void;
  onLocate: (path: string) => void;
  onResetPath: () => void;
  optionalNoteKey?: TranslationKey;
}

const STAGE_LABEL = {
  downloading: 'settings.toolStageDownloading',
  extracting: 'settings.toolStageExtracting',
  verifying: 'settings.toolStageVerifying',
  done: 'settings.toolStageVerifying',
} as const satisfies Record<ToolInstallProgress['stage'], TranslationKey>;

/**
 * What an install is doing right now. The byte count only means something
 * while downloading; extracting and the start-up check that follows have no
 * measurable progress, and without saying so a full bar looks like a hang.
 */
export function InstallProgress({
  progress,
  className,
}: {
  progress: ToolInstallProgress;
  className?: string;
}) {
  const { t } = useTranslation();
  const downloading = progress.stage === 'downloading';
  const percent =
    downloading && progress.totalBytes != null && progress.totalBytes > 0
      ? (progress.receivedBytes / progress.totalBytes) * 100
      : null;

  return (
    <div className={className}>
      <Progress value={percent} />
      <div className="mt-1.5 flex items-center gap-2 text-[11.5px] text-fg-muted">
        <span>{t(STAGE_LABEL[progress.stage])}</span>
        {downloading && progress.receivedBytes > 0 && (
          <span className="tabular ml-auto">
            {formatBytes(progress.receivedBytes)}
            {progress.totalBytes != null && ` / ${formatBytes(progress.totalBytes)}`}
          </span>
        )}
      </div>
    </div>
  );
}

const SOURCE_LABEL = {
  managed: 'settings.toolSourceManaged',
  system: 'settings.toolSourceSystem',
  custom: 'settings.toolSourceCustom',
  bundled: 'settings.toolSourceBundled',
  missing: 'settings.toolMissing',
} as const satisfies Record<ToolStatus['source'], TranslationKey>;

export function ToolCard({
  status,
  titleKey,
  hintKey,
  installing,
  customPath,
  onInstall,
  onLocate,
  onResetPath,
  optionalNoteKey,
}: ToolCardProps) {
  const { t } = useTranslation();

  const pickFile = async () => {
    const selected = await open({
      multiple: false,
      directory: false,
      filters: [{ name: 'Executable', extensions: ['exe'] }],
    });
    if (typeof selected === 'string') onLocate(selected);
  };

  return (
    <div className="px-4 py-4">
      <div className="flex items-start gap-3">
        <span
          className={cn(
            'mt-0.5 flex size-8 shrink-0 items-center justify-center rounded-lg',
            status.available ? 'bg-success-soft text-success' : 'bg-warning-soft text-warning',
          )}
        >
          {status.available ? <CheckCircle2 size={17} /> : <CircleAlert size={17} />}
        </span>

        <div className="min-w-0 flex-1">
          <div className="flex flex-wrap items-center gap-2">
            <span className="text-[13.5px] font-medium text-fg">{t(titleKey)}</span>
            {status.available ? (
              <>
                {status.version && <Badge tone="success">{status.version}</Badge>}
                <Badge tone="outline">{t(SOURCE_LABEL[status.source])}</Badge>
              </>
            ) : (
              <Badge tone="warning">{t('settings.toolMissing')}</Badge>
            )}
          </div>

          <p className="mt-1 text-[12.5px] leading-relaxed text-fg-muted">{t(hintKey)}</p>

          {!status.available && optionalNoteKey && (
            <p className="mt-1.5 text-[12.5px] leading-relaxed text-warning">
              {t(optionalNoteKey)}
            </p>
          )}

          {status.path && (
            <Tooltip label={status.path}>
              <p className="mt-1.5 truncate font-mono text-[11.5px] text-fg-faint">
                {truncateMiddle(status.path, 56)}
              </p>
            </Tooltip>
          )}

          {installing && <InstallProgress progress={installing} className="mt-3" />}

          {/* A tool that ships inside the app has nothing to install, and on a
              phone a file the user points at would not be allowed to run. */}
          {!installing && !(status.source === 'bundled' && status.available) && (
            <div className="mt-3 flex flex-wrap items-center gap-2">
              <Button
                size="sm"
                variant={status.available ? 'secondary' : 'primary'}
                icon={status.available ? <RotateCw size={13} /> : <Download size={13} />}
                onClick={onInstall}
              >
                {status.available ? t('settings.toolUpdate') : t('settings.toolInstall')}
              </Button>
              {!IS_MOBILE && (
                <Button
                  size="sm"
                  variant="ghost"
                  icon={<FolderSearch size={13} />}
                  onClick={pickFile}
                >
                  {t('settings.toolLocate')}
                </Button>
              )}
              {customPath && !IS_MOBILE && (
                <Button size="sm" variant="ghost" onClick={onResetPath}>
                  {t('settings.toolReset')}
                </Button>
              )}
            </div>
          )}
        </div>
      </div>
    </div>
  );
}

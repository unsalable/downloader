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

const SOURCE_LABEL = {
  managed: 'settings.toolSourceManaged',
  system: 'settings.toolSourceSystem',
  custom: 'settings.toolSourceCustom',
  bundled: 'settings.toolSourceManaged',
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

  const percent =
    installing?.totalBytes != null && installing.totalBytes > 0
      ? (installing.receivedBytes / installing.totalBytes) * 100
      : null;

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

          {installing && (
            <div className="mt-3">
              <Progress value={percent} />
              <div className="mt-1.5 flex items-center gap-2 text-[11.5px] text-fg-muted">
                <span>{t('settings.toolInstalling')}</span>
                <span className="tabular ml-auto">
                  {formatBytes(installing.receivedBytes)}
                  {installing.totalBytes != null && ` / ${formatBytes(installing.totalBytes)}`}
                </span>
              </div>
            </div>
          )}

          {!installing && (
            <div className="mt-3 flex flex-wrap items-center gap-2">
              <Button
                size="sm"
                variant={status.available ? 'secondary' : 'primary'}
                icon={status.available ? <RotateCw size={13} /> : <Download size={13} />}
                onClick={onInstall}
              >
                {status.available ? t('settings.toolUpdate') : t('settings.toolInstall')}
              </Button>
              <Button
                size="sm"
                variant="ghost"
                icon={<FolderSearch size={13} />}
                onClick={pickFile}
              >
                {t('settings.toolLocate')}
              </Button>
              {customPath && (
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

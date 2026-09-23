import { open } from '@tauri-apps/plugin-dialog';
import { Check } from 'lucide-react';
import { AnimatePresence, motion } from 'motion/react';

import { Button } from '@/components/ui/Button';
import { InlineNotice } from '@/components/ui/InlineNotice';
import { Progress } from '@/components/ui/Progress';
import { Tooltip } from '@/components/ui/Tooltip';
import { errorMessage, useTranslation } from '@/i18n';
import type { TranslationKey } from '@/i18n';
import { cn } from '@/lib/cn';
import { formatBytes, truncateMiddle } from '@/lib/format';
import { FADE } from '@/lib/motion';
import { IS_MOBILE } from '@/lib/platform';
import type { ToolCheckState } from '@/stores/useToolsStore';
import type { AppErrorInfo, ToolInstallProgress, ToolStatus } from '@/types';

interface ToolCardProps {
  status: ToolStatus;
  titleKey: TranslationKey;
  hintKey: TranslationKey;
  installing: ToolInstallProgress | undefined;
  /** Why the last install failed, shown in the row until the next attempt. */
  installError?: string | null;
  /** Installs a missing tool. */
  onInstall: () => void;
  /** Asks whether an installed tool has a newer release, and fetches it if so. */
  onCheck: () => void;
  /** What that last asking found. */
  check?: ToolCheckState;
  optionalNoteKey?: TranslationKey;
  /**
   * A tool the app can point at a copy of its own. Left out for one the app
   * only ever manages itself, which has no setting to hold the chosen path and
   * so no honest way to offer the choice.
   */
  customPath?: string | null;
  onLocate?: (path: string) => void;
  onResetPath?: () => void;
  /**
   * Whether an ordinary download works without this tool. When it does, the
   * missing state drops the warning colour: the row is an offer, and dressing
   * it as a fault makes a perfectly working app look broken.
   */
  optional?: boolean;
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
      <div className="mt-1.5 flex items-center gap-3 text-[12.5px] text-fg-muted">
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

/** Why a check failed, in as few words as there are for it. */
function checkFailureReason(error: AppErrorInfo, t: (key: TranslationKey) => string): string {
  // Only a connection problem has something the user can do about it; what
  // else can go wrong -- GitHub rate-limiting or unwell -- passes by itself.
  const connection = ['network', 'offline', 'networkBlocked'].includes(error.code);
  return connection ? errorMessage(error) : t('settings.toolCheckLater');
}

/** One tool as a row of a `ListGroup`: what it is, its state, what can be done. */
export function ToolCard({
  status,
  titleKey,
  hintKey,
  installing,
  installError,
  customPath,
  onInstall,
  onCheck,
  check,
  onLocate,
  onResetPath,
  optionalNoteKey,
  optional,
}: ToolCardProps) {
  const { t } = useTranslation();
  const quietlyMissing = !status.available && optional === true;
  const checking = status.available && check?.checking === true;
  // Said only of the copy that was asked about. A different version since --
  // installed from another screen, or a file chosen by hand -- was not.
  const upToDate =
    status.available &&
    check?.result?.upToDate === true &&
    check.result.installed === status.version;
  // The sizes a `SettingRow` sets its two lines in, so the rows of a group agree.
  const titleSize = IS_MOBILE ? 'text-[15px]' : 'text-[13.5px]';
  const bodySize = IS_MOBILE ? 'text-[13px]' : 'text-[12.5px]';

  const pickFile = async () => {
    if (!onLocate) return;
    const selected = await open({
      multiple: false,
      directory: false,
      filters: [{ name: 'Executable', extensions: ['exe'] }],
    });
    if (typeof selected === 'string') onLocate(selected);
  };

  return (
    <div className={cn('px-4', IS_MOBILE ? 'py-4' : 'py-3.5')}>
      <div className="flex items-center justify-between gap-3">
        <span className={cn('min-w-0 text-fg', titleSize)}>{t(titleKey)}</span>
        {status.available ? (
          <span className={cn('tabular flex min-w-0 max-w-[55%] items-center gap-1.5 text-fg-muted', bodySize)}>
            <Check size={14} aria-hidden="true" className="shrink-0 text-success" />
            {status.version && <span className="truncate">{status.version}</span>}
            {/* The answer to "check for update" when there was nothing to
                fetch. It stays for the session: the question was answered. */}
            <AnimatePresence initial={false}>
              {upToDate && (
                <motion.span
                  key="current"
                  variants={FADE}
                  initial="initial"
                  animate="animate"
                  exit="exit"
                  className="flex shrink-0 items-center gap-1.5"
                >
                  <span aria-hidden="true">·</span>
                  {t('settings.toolUpToDate')}
                </motion.span>
              )}
            </AnimatePresence>
          </span>
        ) : (
          <span className={cn('shrink-0', bodySize, quietlyMissing ? 'text-fg-muted' : 'text-warning')}>
            {t('settings.toolMissing')}
          </span>
        )}
      </div>

      <p className={cn('mt-0.5 leading-relaxed text-fg-muted', bodySize)}>{t(hintKey)}</p>

      {!status.available && optionalNoteKey && (
        <p
          className={cn(
            'mt-1.5 leading-relaxed',
            bodySize,
            quietlyMissing ? 'text-fg-muted' : 'text-warning',
          )}
        >
          {t(optionalNoteKey)}
        </p>
      )}

      {status.available && (
        <div className={cn('mt-1.5 flex min-w-0 flex-wrap items-baseline gap-x-3 text-fg-muted', bodySize)}>
          <span>{t(SOURCE_LABEL[status.source])}</span>
          {status.path && (
            <Tooltip label={status.path}>
              <span className="selectable min-w-0 truncate font-mono text-[12px] text-fg-faint">
                {truncateMiddle(status.path, 56)}
              </span>
            </Tooltip>
          )}
        </div>
      )}

      {installing && <InstallProgress progress={installing} className="mt-3" />}

      {installError != null && !installing && (
        <InlineNotice tone="error" className="mt-2.5">
          <span className="block font-medium">{t('settings.toolInstallFailed')}</span>
          {installError}
        </InlineNotice>
      )}

      {check?.error && status.available && !installing && !checking && (
        <InlineNotice tone="error" className="mt-2.5">
          <span className="block font-medium">{t('settings.toolCheckFailed')}</span>
          {checkFailureReason(check.error, t)}
        </InlineNotice>
      )}

      {/* A tool that ships inside the app has nothing to install, and on a
          phone a file the user points at would not be allowed to run. */}
      {!installing && !(status.source === 'bundled' && status.available) && (
        <div className="mt-3 flex flex-wrap items-center gap-2">
          {/* An installed tool is asked about before anything is fetched; a
              missing one has nothing to compare and is simply installed. */}
          <Button
            size="sm"
            variant={status.available || quietlyMissing ? 'secondary' : 'primary'}
            loading={checking}
            onClick={status.available ? onCheck : onInstall}
          >
            {status.available
              ? t(checking ? 'settings.toolChecking' : 'settings.toolUpdate')
              : t('settings.toolInstall')}
          </Button>
          {onLocate && !IS_MOBILE && (
            <Button size="sm" variant="ghost" onClick={pickFile}>
              {t('settings.toolLocate')}
            </Button>
          )}
          {customPath && onResetPath && !IS_MOBILE && (
            <Button size="sm" variant="ghost" onClick={onResetPath}>
              {t('settings.toolReset')}
            </Button>
          )}
        </div>
      )}
    </div>
  );
}

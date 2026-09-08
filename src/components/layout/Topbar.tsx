import { Monitor, Moon, Settings as SettingsIcon, Sun } from 'lucide-react';

import { IconButton } from '@/components/ui/IconButton';
import { Progress } from '@/components/ui/Progress';
import { Tooltip } from '@/components/ui/Tooltip';
import { useTranslation } from '@/i18n';
import { cn } from '@/lib/cn';
import { formatSpeed } from '@/lib/format';
import type { DownloadTask, ThemePreference } from '@/types';
import type { Route } from './Sidebar';

const TITLES: Record<Route, string> = {
  home: 'app.name',
  downloads: 'downloads.title',
  convert: 'convert.title',
  history: 'history.title',
  settings: 'settings.title',
  about: 'nav.about',
};

const THEME_ORDER: ThemePreference[] = ['dark', 'light', 'system'];
const THEME_ICON = { dark: Moon, light: Sun, system: Monitor } as const;

interface TopbarProps {
  route: Route;
  theme: ThemePreference;
  onThemeChange: (theme: ThemePreference) => void;
  onOpenSettings: () => void;
  onOpenDownloads: () => void;
  activeTasks: DownloadTask[];
}

export function Topbar({
  route,
  theme,
  onThemeChange,
  onOpenSettings,
  onOpenDownloads,
  activeTasks,
}: TopbarProps) {
  const { t } = useTranslation();
  const ThemeIcon = THEME_ICON[theme];

  const totalSpeed = activeTasks.reduce((sum, task) => sum + task.progress.speedBps, 0);
  const overallPercent =
    activeTasks.length > 0
      ? activeTasks.reduce((sum, task) => sum + (task.progress.percent ?? 0), 0) /
        activeTasks.length
      : null;

  return (
    <header
      className={cn(
        'relative z-20 flex h-14 shrink-0 items-center gap-3 border-b border-[var(--border)]',
        'bg-bg/80 px-5 backdrop-blur-xl',
      )}
    >
      <h1 className="font-mono text-[11px] font-semibold uppercase tracking-[0.12em] text-fg">
        {t(TITLES[route] as never)}
      </h1>

      <div className="ml-auto flex items-center gap-1.5">
        {activeTasks.length > 0 && (
          <Tooltip label={t('topbar.activeCount', { n: activeTasks.length })}>
            <button
              type="button"
              onClick={onOpenDownloads}
              className={cn(
                'mr-1 flex h-8 items-center gap-2.5 rounded-md border border-[var(--border)]',
                'bg-surface px-2.5 transition-colors duration-150 hover:bg-surface-hover',
              )}
            >
              <span className="size-1.5 shrink-0 bg-accent" />
              <div className="w-16">
                <Progress value={overallPercent} size="sm" />
              </div>
              <span className="metric w-[70px] text-right text-[11.5px] text-fg-muted">
                {formatSpeed(totalSpeed)}
              </span>
            </button>
          </Tooltip>
        )}

        <IconButton
          icon={<ThemeIcon size={16} />}
          label={`${t('topbar.theme')}: ${t(`topbar.theme${theme[0]!.toUpperCase()}${theme.slice(1)}` as never)}`}
          onClick={() => {
            const index = THEME_ORDER.indexOf(theme);
            onThemeChange(THEME_ORDER[(index + 1) % THEME_ORDER.length]!);
          }}
        />

        <IconButton
          icon={<SettingsIcon size={16} />}
          label={t('topbar.settings')}
          active={route === 'settings'}
          onClick={onOpenSettings}
        />
      </div>
    </header>
  );
}

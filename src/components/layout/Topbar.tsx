import { Info, Monitor, Moon, Settings as SettingsIcon, Sun } from 'lucide-react';

import { IconButton } from '@/components/ui/IconButton';
import { Progress } from '@/components/ui/Progress';
import { Tooltip } from '@/components/ui/Tooltip';
import { useMemo } from 'react';
import { useShallow } from 'zustand/react/shallow';

import { useTranslation } from '@/i18n';
import { cn } from '@/lib/cn';
import { formatSpeed } from '@/lib/format';
import { IS_MOBILE } from '@/lib/platform';
import { selectActive, useQueueStore } from '@/stores/useQueueStore';
import type { ThemePreference } from '@/types';
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
  onOpenAbout: () => void;
  onOpenDownloads: () => void;
}

export function Topbar({
  route,
  theme,
  onThemeChange,
  onOpenSettings,
  onOpenAbout,
  onOpenDownloads,
}: TopbarProps) {
  const { t } = useTranslation();
  const ThemeIcon = THEME_ICON[theme];

  return (
    <header
      className={cn(
        'relative z-20 flex h-14 shrink-0 items-center gap-3 border-b border-[var(--border)]',
        // Nothing scrolls under a phone's bar, and a blur there would only be
        // re-rendering the backdrop.
        IS_MOBILE ? 'bg-bg px-4' : 'bg-bg/80 px-5 backdrop-blur-xl',
      )}
    >
      <h1 className="truncate font-mono text-[11px] font-semibold uppercase tracking-[0.12em] text-fg">
        {t(TITLES[route] as never)}
      </h1>

      <div className="ml-auto flex shrink-0 items-center gap-1.5">
        <ActivityIndicator onOpenDownloads={onOpenDownloads} />

        <IconButton
          icon={<ThemeIcon size={16} />}
          label={`${t('topbar.theme')}: ${t(`topbar.theme${theme[0]!.toUpperCase()}${theme.slice(1)}` as never)}`}
          onClick={() => {
            const index = THEME_ORDER.indexOf(theme);
            onThemeChange(THEME_ORDER[(index + 1) % THEME_ORDER.length]!);
          }}
        />

        {/* Settings has its own tab at the bottom of a phone screen; About,
            which does not, takes this place instead. */}
        {IS_MOBILE ? (
          <IconButton
            icon={<Info size={16} />}
            label={t('nav.about')}
            active={route === 'about'}
            onClick={onOpenAbout}
          />
        ) : (
          <IconButton
            icon={<SettingsIcon size={16} />}
            label={t('topbar.settings')}
            active={route === 'settings'}
            onClick={onOpenSettings}
          />
        )}
      </div>
    </header>
  );
}

/**
 * The live transfer rate. It subscribes to the queue itself, so the progress
 * ticks that keep it current re-render this pill and not the bar around it.
 */
function ActivityIndicator({ onOpenDownloads }: { onOpenDownloads: () => void }) {
  const { t } = useTranslation();
  const activeTasks = useQueueStore(useShallow((state) => selectActive(state.tasks)));

  const { totalSpeed, overallPercent } = useMemo(
    () => ({
      totalSpeed: activeTasks.reduce((sum, task) => sum + task.progress.speedBps, 0),
      overallPercent:
        activeTasks.length > 0
          ? activeTasks.reduce((sum, task) => sum + (task.progress.percent ?? 0), 0) /
            activeTasks.length
          : null,
    }),
    [activeTasks],
  );

  if (activeTasks.length === 0) return null;

  return (
    <Tooltip label={t('topbar.activeCount', { n: activeTasks.length })}>
      <button
        type="button"
        onClick={onOpenDownloads}
        className={cn(
          'pressable mr-1 flex h-8 items-center gap-2.5 rounded-md border border-[var(--border)]',
          'bg-surface px-2.5 hover:bg-surface-hover',
        )}
      >
        <span className="size-1.5 shrink-0 bg-accent" />
        {/* A phone's top bar has room for the rate or the bar, not both. */}
        {!IS_MOBILE && (
          <div className="w-16">
            <Progress value={overallPercent} size="sm" />
          </div>
        )}
        <span className="metric w-[70px] text-right text-[11.5px] text-fg-muted">
          {formatSpeed(totalSpeed)}
        </span>
      </button>
    </Tooltip>
  );
}

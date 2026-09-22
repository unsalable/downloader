import { Clock, Download, House, Repeat, Settings as SettingsIcon, type LucideIcon } from 'lucide-react';

import { useTranslation } from '@/i18n';
import type { TranslationKey } from '@/i18n';
import { cn } from '@/lib/cn';
import { badgeFor, type Route } from './Sidebar';

const ENTRIES: { route: Route; label: TranslationKey; icon: LucideIcon }[] = [
  { route: 'home', label: 'nav.home', icon: House },
  { route: 'downloads', label: 'nav.downloads', icon: Download },
  { route: 'convert', label: 'nav.convert', icon: Repeat },
  { route: 'history', label: 'nav.history', icon: Clock },
  { route: 'settings', label: 'nav.settings', icon: SettingsIcon },
];

interface BottomNavProps {
  route: Route;
  onNavigate: (route: Route) => void;
  activeCount: number;
  convertingCount: number;
}

/**
 * The phone's navigation: the sidebar's destinations along the bottom, where a
 * thumb reaches them. About moves to the top bar, since five is as many as fit.
 *
 * The selected tab is told the way the sidebar tells it: the icon takes the
 * accent and the label the primary colour. Nothing else marks it.
 */
export function BottomNav({ route, onNavigate, activeCount, convertingCount }: BottomNavProps) {
  const { t } = useTranslation();

  return (
    <nav
      aria-label={t('app.name')}
      className="relative z-20 flex h-[60px] shrink-0 border-t border-[var(--border)] bg-surface-sunken"
    >
      {ENTRIES.map((entry) => {
        const Icon = entry.icon;
        const active = route === entry.route;
        const badge = badgeFor(entry.route, activeCount, convertingCount);

        return (
          <button
            key={entry.route}
            type="button"
            onClick={() => onNavigate(entry.route)}
            aria-current={active ? 'page' : undefined}
            className={cn(
              'relative flex min-w-0 flex-1 flex-col items-center justify-center gap-[3px]',
              'text-[12px] font-medium transition-colors duration-150 ease-out-quint',
              // A finger gets no hover, so the tab answers the touch itself.
              'active:bg-fill',
              active ? 'text-fg' : 'text-fg-muted',
            )}
          >
            <span className="relative">
              <Icon size={21} className={cn(active && 'text-accent')} />
              {badge > 0 && (
                <span className="tabular absolute -right-3.5 -top-1.5 min-w-[17px] rounded-full bg-accent px-[5px] text-center text-[12px] font-semibold leading-[17px] text-accent-fg">
                  {badge > 99 ? '99+' : badge}
                </span>
              )}
            </span>
            <span className="max-w-full truncate px-1 leading-4">{t(entry.label)}</span>
          </button>
        );
      })}
    </nav>
  );
}

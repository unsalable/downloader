import { motion } from 'motion/react';
import { Clock, Download, House, Repeat, Settings as SettingsIcon, type LucideIcon } from 'lucide-react';

import { useTranslation } from '@/i18n';
import type { TranslationKey } from '@/i18n';
import { cn } from '@/lib/cn';
import { SPRING } from '@/lib/motion';
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
 */
export function BottomNav({ route, onNavigate, activeCount, convertingCount }: BottomNavProps) {
  const { t } = useTranslation();

  return (
    <nav
      aria-label={t('app.name')}
      className="relative z-20 flex h-[62px] shrink-0 border-t border-[var(--border)] bg-surface-sunken"
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
              'relative flex min-w-0 flex-1 flex-col items-center justify-center gap-1',
              'text-[10.5px] font-medium transition-colors duration-150 ease-out-quint',
              // A finger gets no hover, so the tab answers the touch itself.
              'active:bg-surface-hover',
              active ? 'text-fg' : 'text-fg-muted',
            )}
          >
            {active && (
              <motion.span
                layoutId="bottom-nav-active"
                transition={SPRING.glide}
                className="absolute inset-x-4 top-0 h-[2px] rounded-full bg-[var(--accent)]"
              />
            )}
            <span className="relative">
              <Icon size={19} className={cn(active && 'text-accent')} />
              {badge > 0 && (
                <span className="metric absolute -right-3 -top-1.5 rounded-[4px] bg-accent px-1 text-[9.5px] font-semibold leading-[14px] text-accent-fg">
                  {badge > 99 ? '99+' : badge}
                </span>
              )}
            </span>
            <span className="max-w-full truncate px-1">{t(entry.label)}</span>
          </button>
        );
      })}
    </nav>
  );
}

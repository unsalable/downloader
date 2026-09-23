import { motion } from 'motion/react';
import {
  Clock,
  Download,
  House,
  Info,
  Film,
  PanelLeft,
  Repeat,
  Settings as SettingsIcon,
  type LucideIcon,
} from 'lucide-react';

import { Tooltip } from '@/components/ui/Tooltip';
import { useTranslation } from '@/i18n';
import { cn } from '@/lib/cn';
import { SPRING } from '@/lib/motion';
import type { TranslationKey } from '@/i18n';
import { Logo } from './Logo';

export type Route = 'home' | 'downloads' | 'convert' | 'editor' | 'history' | 'settings' | 'about';

interface NavEntry {
  route: Route;
  label: TranslationKey;
  icon: LucideIcon;
}

const PRIMARY: NavEntry[] = [
  { route: 'home', label: 'nav.home', icon: House },
  { route: 'downloads', label: 'nav.downloads', icon: Download },
  { route: 'convert', label: 'nav.convert', icon: Repeat },
  { route: 'editor', label: 'nav.editor', icon: Film },
  { route: 'history', label: 'nav.history', icon: Clock },
];

const SECONDARY: NavEntry[] = [
  { route: 'settings', label: 'nav.settings', icon: SettingsIcon },
  { route: 'about', label: 'nav.about', icon: Info },
];

interface SidebarProps {
  route: Route;
  onNavigate: (route: Route) => void;
  collapsed: boolean;
  onToggleCollapsed: () => void;
  activeCount: number;
  convertingCount: number;
}

export function Sidebar({
  route,
  onNavigate,
  collapsed,
  onToggleCollapsed,
  activeCount,
  convertingCount,
}: SidebarProps) {
  const { t } = useTranslation();

  return (
    <nav
      aria-label={t('app.name')}
      className={cn(
        'relative z-20 flex shrink-0 flex-col border-r border-[var(--border)] bg-surface-sunken',
        'transition-[width] duration-350 ease-out-quint',
        collapsed ? 'w-[57px]' : 'w-[217px]',
      )}
    >
      {/* Padded, and the rail sized, so the mark stands over the column of icons
          beneath it and neither moves sideways when the sidebar collapses. */}
      <div className={cn('flex h-[52px] items-center gap-2.5 px-[17px]', collapsed && 'justify-center px-0')}>
        <Logo size={22} className="shrink-0" />
        {!collapsed && (
          // The name is English whatever language the interface is in, and says
          // so, so that it is read out -- and cased -- as English.
          <span lang="en" className="truncate text-[13px] font-semibold text-fg">
            {t('app.name')}
          </span>
        )}
      </div>

      <div className="flex flex-1 flex-col gap-0.5 px-2.5 pt-1">
        {PRIMARY.map((entry) => (
          <NavButton
            key={entry.route}
            entry={entry}
            active={route === entry.route}
            collapsed={collapsed}
            badge={badgeFor(entry.route, activeCount, convertingCount)}
            onClick={() => onNavigate(entry.route)}
          />
        ))}
      </div>

      {/* The app's own pages sit at the foot of the list, apart from the places
          where work happens; the distance does what a rule would. */}
      <div className="flex flex-col gap-0.5 px-2.5 pb-2.5">
        {SECONDARY.map((entry) => (
          <NavButton
            key={entry.route}
            entry={entry}
            active={route === entry.route}
            collapsed={collapsed}
            badge={0}
            onClick={() => onNavigate(entry.route)}
          />
        ))}

        <Tooltip label={collapsed ? t('nav.expand') : t('nav.collapse')} side="right">
          <button
            type="button"
            onClick={onToggleCollapsed}
            aria-label={collapsed ? t('nav.expand') : t('nav.collapse')}
            aria-expanded={!collapsed}
            className={cn(
              'pressable-sm flex h-[34px] items-center rounded-[8px] px-2.5 text-fg-faint',
              'hover:bg-fill hover:text-fg-muted',
              collapsed && 'justify-center px-0',
            )}
          >
            <PanelLeft size={16} />
          </button>
        </Tooltip>
      </div>
    </nav>
  );
}

/** Only the two screens that do background work carry a count. */
export function badgeFor(route: Route, downloading: number, converting: number): number {
  if (route === 'downloads') return downloading;
  if (route === 'convert') return converting;
  return 0;
}

function NavButton({
  entry,
  active,
  collapsed,
  badge,
  onClick,
}: {
  entry: NavEntry;
  active: boolean;
  collapsed: boolean;
  badge: number;
  onClick: () => void;
}) {
  const { t } = useTranslation();
  const Icon = entry.icon;
  const label = t(entry.label);

  const button = (
    <button
      type="button"
      onClick={onClick}
      aria-current={active ? 'page' : undefined}
      className={cn(
        'relative flex h-[34px] items-center gap-2.5 rounded-[8px] px-2.5 text-[13px] font-medium',
        'transition-colors duration-150 ease-out-quint',
        collapsed && 'justify-center px-0',
        active ? 'text-fg' : 'text-fg-muted hover:bg-fill hover:text-fg',
      )}
    >
      {active && (
        <motion.span
          // One shared element slides between items, so switching pages reads
          // as the selection moving rather than two highlights crossfading.
          layoutId="sidebar-active"
          transition={SPRING.glide}
          className="absolute inset-0 rounded-[8px] bg-fill-active"
        />
      )}
      <Icon size={16} className={cn('relative z-10 shrink-0', active && 'text-accent')} />
      {!collapsed && <span className="relative z-10 truncate">{label}</span>}

      {badge > 0 &&
        (collapsed ? (
          // The rail has no room for a figure, so it only says there is one;
          // a screen reader still gets the figure.
          <span className="absolute right-[7px] top-[6px] z-10 size-[7px] rounded-full bg-accent">
            <span className="sr-only">{badge}</span>
          </span>
        ) : (
          <span className="tabular relative z-10 ml-auto min-w-5 rounded-full bg-fill-hover px-1.5 text-center text-[12px] font-medium leading-5 text-fg">
            {badge > 99 ? '99+' : badge}
          </span>
        ))}
    </button>
  );

  return collapsed ? (
    <Tooltip label={label} side="right">
      {button}
    </Tooltip>
  ) : (
    button
  );
}

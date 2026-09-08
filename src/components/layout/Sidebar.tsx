import { motion } from 'motion/react';
import {
  ChevronsLeft,
  Clock,
  Download,
  House,
  Info,
  Repeat,
  Settings as SettingsIcon,
  type LucideIcon,
} from 'lucide-react';

import { Tooltip } from '@/components/ui/Tooltip';
import { useTranslation } from '@/i18n';
import { cn } from '@/lib/cn';
import type { TranslationKey } from '@/i18n';
import { Logo } from './Logo';

export type Route = 'home' | 'downloads' | 'convert' | 'history' | 'settings' | 'about';

interface NavEntry {
  route: Route;
  label: TranslationKey;
  icon: LucideIcon;
}

const PRIMARY: NavEntry[] = [
  { route: 'home', label: 'nav.home', icon: House },
  { route: 'downloads', label: 'nav.downloads', icon: Download },
  { route: 'convert', label: 'nav.convert', icon: Repeat },
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
        'transition-[width] duration-300 ease-[var(--ease-out-quint)]',
        collapsed ? 'w-[62px]' : 'w-[212px]',
      )}
    >
      <div className={cn('flex h-14 items-center gap-2.5 px-4', collapsed && 'justify-center px-0')}>
        <Logo size={22} className="shrink-0" />
        {!collapsed && (
          <span className="truncate font-mono text-[11px] font-semibold uppercase leading-tight tracking-[0.14em] text-fg">
            Universal
            <span className="block text-fg-faint">Downloader</span>
          </span>
        )}
      </div>

      <div className="flex flex-1 flex-col gap-0.5 px-2.5 pt-2">
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

        <div className="mx-1 my-2.5 h-px bg-[var(--border)]" />

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
      </div>

      <div className="p-2.5">
        <Tooltip label={collapsed ? t('nav.expand') : t('nav.collapse')} side="right">
          <button
            type="button"
            onClick={onToggleCollapsed}
            aria-label={collapsed ? t('nav.expand') : t('nav.collapse')}
            className={cn(
              'flex h-8 w-full items-center justify-center rounded-lg text-fg-faint',
              'transition-colors duration-150 hover:bg-surface-hover hover:text-fg-muted',
            )}
          >
            <ChevronsLeft
              size={15}
              className={cn('transition-transform duration-300', collapsed && 'rotate-180')}
            />
          </button>
        </Tooltip>
      </div>
    </nav>
  );
}

/** Only the two screens that do background work carry a count. */
function badgeFor(route: Route, downloading: number, converting: number): number {
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
        'group relative flex h-9 items-center gap-2.5 rounded-[6px] px-2.5 text-[13px] font-medium',
        'transition-colors duration-150',
        collapsed && 'justify-center px-0',
        active ? 'text-fg' : 'text-fg-muted hover:bg-surface-hover hover:text-fg',
      )}
    >
      {active && (
        <motion.span
          // One shared element slides between items, so switching pages reads
          // as the indicator moving rather than two highlights crossfading.
          layoutId="sidebar-active"
          transition={{ type: 'spring', stiffness: 480, damping: 40, mass: 0.7 }}
          className="absolute inset-0 rounded-[6px] bg-surface-active before:absolute before:inset-y-1.5 before:-left-2.5 before:w-[2px] before:rounded-full before:bg-[var(--accent)] before:content-['']"
        />
      )}
      <Icon size={16} className={cn('relative z-10 shrink-0', active && 'text-accent')} />
      {!collapsed && <span className="relative z-10 truncate">{label}</span>}

      {badge > 0 && (
        <span
          className={cn(
            'relative z-10 metric ml-auto rounded-[4px] bg-accent px-1.5 text-[10px]',
            'font-semibold leading-[16px] text-accent-fg',
            collapsed && 'absolute -right-0.5 -top-0.5 ml-0 px-1',
          )}
        >
          {badge > 99 ? '99+' : badge}
        </span>
      )}
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

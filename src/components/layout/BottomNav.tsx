import { motion, type Variants } from 'motion/react';
import { Download, Film, House, Repeat, Settings as SettingsIcon, type LucideIcon } from 'lucide-react';
import { memo } from 'react';

import { useTranslation } from '@/i18n';
import type { TranslationKey } from '@/i18n';
import { cn } from '@/lib/cn';
import { SPRING, T } from '@/lib/motion';
import { badgeFor, type Route } from './Sidebar';

const ENTRIES: { route: Route; label: TranslationKey; icon: LucideIcon }[] = [
  { route: 'home', label: 'nav.home', icon: House },
  { route: 'downloads', label: 'nav.downloads', icon: Download },
  { route: 'editor', label: 'nav.editor', icon: Film },
  { route: 'convert', label: 'nav.convert', icon: Repeat },
  { route: 'settings', label: 'nav.settings', icon: SettingsIcon },
];

/**
 * The tabs in the order they stand along the bar, left to right. A screen
 * change follows this order, so the app reads it from here rather than keeping
 * a second copy that could drift.
 */
export const PHONE_TABS: readonly Route[] = ENTRIES.map((entry) => entry.route);

/**
 * A finger gets no hover, so the tab answers the touch itself: what it holds
 * gives a little while pressed. The label is propagated from the button's
 * `whileTap`, which lets the content shrink without the indicator behind it,
 * whose glide would be thrown off by a scaled parent.
 */
const PRESS: Variants = { pressed: { scale: 0.94, transition: T.micro } };

interface BottomNavProps {
  /** The tab to show as selected: About, opened over a tab, leaves it selected. */
  route: Route;
  onNavigate: (route: Route) => void;
  activeCount: number;
  convertingCount: number;
}

/**
 * The phone's navigation: the places where work happens, along the bottom
 * where a thumb reaches them. History is the second half of Downloads here, and
 * About moves to the top bar, since five is as many as fit.
 *
 * The selected tab sits in a soft pill that glides from tab to tab, so a switch
 * reads as the selection moving rather than one highlight going out and
 * another coming on. The icon takes the accent, as it does in the sidebar.
 *
 * Memoised: the shell re-renders for reasons that have nothing to do with the
 * bar, and the bar's props change only on a switch or when a count moves.
 */
export const BottomNav = memo(function BottomNav({
  route,
  onNavigate,
  activeCount,
  convertingCount,
}: BottomNavProps) {
  const { t } = useTranslation();

  return (
    <nav
      aria-label={t('app.name')}
      // The page's own colour, because Android paints the system navigation
      // bar under this one with it: a fill of its own left a faint band
      // between the two. The hairline alone says where the bar begins.
      className="relative z-20 flex h-16 shrink-0 border-t border-[var(--border)] bg-bg"
    >
      {ENTRIES.map((entry) => {
        const Icon = entry.icon;
        const active = route === entry.route;
        const badge = badgeFor(entry.route, activeCount, convertingCount);

        return (
          <motion.button
            key={entry.route}
            type="button"
            onClick={() => onNavigate(entry.route)}
            aria-current={active ? 'page' : undefined}
            whileTap="pressed"
            className={cn(
              'relative flex min-w-0 flex-1 flex-col items-center justify-center gap-1',
              'text-[12px] font-medium transition-colors duration-150 ease-out-quint',
              active ? 'text-fg' : 'text-fg-muted',
            )}
          >
            {/* Placed where the icon's slot is, and not inside it, so the
                press that shrinks the icon leaves the pill's size alone. */}
            {active && (
              <motion.span
                layoutId="bottom-nav-active"
                transition={SPRING.glide}
                aria-hidden="true"
                className="absolute left-[calc(50%-28px)] top-[calc(50%-25px)] h-[30px] w-14 rounded-full bg-fill-active"
              />
            )}

            <motion.span variants={PRESS} className="relative flex h-[30px] items-center">
              {/* Arriving at the tab, the icon springs up from slightly small:
                  the one movement in the bar that belongs to the new tab
                  rather than to the selection travelling. */}
              <motion.span
                initial={false}
                animate={{ scale: active ? [0.86, 1] : 1 }}
                transition={active ? SPRING.snap : T.micro}
                className="flex"
              >
                <Icon
                  size={21}
                  className={cn(
                    'transition-colors duration-150 ease-out-quint',
                    active ? 'text-accent' : 'text-fg-muted',
                  )}
                />
              </motion.span>
              {badge > 0 && (
                <span className="tabular absolute -right-3.5 top-0 min-w-[17px] rounded-full bg-accent px-[5px] text-center text-[12px] font-semibold leading-[17px] text-accent-fg">
                  {badge > 99 ? '99+' : badge}
                </span>
              )}
            </motion.span>

            <motion.span variants={PRESS} className="relative max-w-full truncate px-1 leading-4">
              {t(entry.label)}
            </motion.span>
          </motion.button>
        );
      })}
    </nav>
  );
});

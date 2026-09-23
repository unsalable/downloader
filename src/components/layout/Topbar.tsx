import { ChevronLeft, Info } from 'lucide-react';

import { IconButton } from '@/components/ui/IconButton';
import { useTranslation } from '@/i18n';
import type { TranslationKey } from '@/i18n';
import { cn } from '@/lib/cn';
import type { Route } from './Sidebar';

const TITLES: Record<Route, TranslationKey> = {
  home: 'app.name',
  // Both halves of it on a phone, the history included.
  downloads: 'downloads.title',
  convert: 'convert.title',
  editor: 'editor.title',
  history: 'history.title',
  settings: 'settings.title',
  about: 'nav.about',
};

interface TopbarProps {
  route: Route;
  onOpenAbout: () => void;
  onBack: () => void;
}

/**
 * The phone's title bar: the name of the screen, and the way to About, which
 * has no tab of its own. The desktop has no bar at all -- each page there opens
 * with its own large title (see `PageHeader`), and the sidebar reaches About.
 *
 * About is opened over whichever tab is showing, and says so the way a section
 * of Settings does: Back in place of the button that opened it.
 */
export function Topbar({ route, onOpenAbout, onBack }: TopbarProps) {
  const { t } = useTranslation();
  const over = route === 'about';

  return (
    // No fill of its own: nothing scrolls beneath it, and with one the glow
    // behind Home would stop at a hard line under the bar.
    <header
      className={cn(
        'relative z-20 flex h-12 shrink-0 items-center',
        over ? 'gap-1 px-1.5' : 'gap-3 pl-4 pr-2',
      )}
    >
      {over && (
        // Matches the Back of a Settings section, so the two pages opened over
        // a tab go back the same way.
        <button
          type="button"
          onClick={onBack}
          aria-label={t('common.back')}
          className="pressable-sm flex size-11 shrink-0 items-center justify-center rounded-full text-fg active:bg-surface-active"
        >
          <ChevronLeft size={24} />
        </button>
      )}

      {/* Home is titled with the app's name, which is English in any language. */}
      <h1
        lang={route === 'home' ? 'en' : undefined}
        className="min-w-0 flex-1 truncate text-[17px] font-semibold tracking-[-0.01em] text-fg"
      >
        {t(TITLES[route])}
      </h1>

      {!over && (
        <IconButton icon={<Info size={19} />} label={t('nav.about')} onClick={onOpenAbout} />
      )}
    </header>
  );
}

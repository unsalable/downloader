import { Info } from 'lucide-react';

import { IconButton } from '@/components/ui/IconButton';
import { useTranslation } from '@/i18n';
import type { TranslationKey } from '@/i18n';
import type { Route } from './Sidebar';

const TITLES: Record<Route, TranslationKey> = {
  home: 'app.name',
  downloads: 'downloads.title',
  convert: 'convert.title',
  history: 'history.title',
  settings: 'settings.title',
  about: 'nav.about',
};

interface TopbarProps {
  route: Route;
  onOpenAbout: () => void;
}

/**
 * The phone's title bar: the name of the screen, and the way to About, which
 * has no tab of its own. The desktop has no bar at all -- each page there opens
 * with its own large title (see `PageHeader`), and the sidebar reaches About.
 */
export function Topbar({ route, onOpenAbout }: TopbarProps) {
  const { t } = useTranslation();

  return (
    // No fill of its own: nothing scrolls beneath it, and with one the glow
    // behind Home would stop at a hard line under the bar.
    <header className="relative z-20 flex h-12 shrink-0 items-center gap-3 pl-4 pr-2">
      {/* Home is titled with the app's name, which is English in any language. */}
      <h1
        lang={route === 'home' ? 'en' : undefined}
        className="min-w-0 flex-1 truncate text-[17px] font-semibold tracking-[-0.01em] text-fg"
      >
        {t(TITLES[route])}
      </h1>

      <IconButton
        icon={<Info size={19} />}
        label={t('nav.about')}
        active={route === 'about'}
        onClick={onOpenAbout}
      />
    </header>
  );
}

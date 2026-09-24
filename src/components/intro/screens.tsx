import { Download, Film, House, Repeat, Settings, type LucideIcon } from 'lucide-react';
import { memo, type CSSProperties, type ReactNode } from 'react';

import { blend, SCREEN } from './kit';
import type { IntroStrings } from './strings';

/*
 * The app as the film's phone shows it: its chrome, redrawn at its real size
 * (390 px across) from the same tokens and the same words, with every moving
 * part a number the scene sets for the frame. The screens themselves live
 * with the scenes that show them.
 *
 * Redrawn rather than reused because the real components are made of stores,
 * the backend and Motion animations, none of which can be told what frame it
 * is. What is here is only the look -- kept close to the real thing on
 * purpose, so the phone after the film looks like the phone in it.
 *
 * No status bar and no title bar: the film keeps the words on screen few, and
 * shows the app's content and its tab bar -- the parts it teaches.
 */

/** Where the app's content sits on the film's screen, in u. */
export const CONTENT = { top: 44, bottom: 758 } as const;
export const TAB_BAR = { top: 758, height: 64 } as const;
/** The system's gesture strip under the app, which the real page is padded clear of. */
export const GESTURE = { top: 822, height: 22 } as const;

const TABS: { key: keyof IntroStrings; icon: LucideIcon }[] = [
  { key: 'navHome', icon: House },
  { key: 'navDownloads', icon: Download },
  { key: 'navEditor', icon: Film },
  { key: 'navConvert', icon: Repeat },
  { key: 'navSettings', icon: Settings },
];

interface TabBarProps {
  strings: IntroStrings;
  /** Where the pill is: a tab's index, or between two while it glides. */
  pill: number;
  /** The tab whose icon is lit. */
  selected: number;
  /**
   * While the selection moves: the tab it is leaving, and how far the change
   * has got, 0..1. The two icons cross over in colour, as the real ones do
   * with their colour transition, instead of swapping in one frame.
   */
  leaving?: number;
  change?: number;
  /** The lit icon's scale, for its arrival spring. */
  selectedScale?: number;
  /** A tab being pressed, and its press scale. */
  pressedIndex?: number;
  pressedScale?: number;
  /** 0 in place; 1 stepped down out of the way, as it does under the editor. */
  away?: number;
}

/**
 * The phone's bottom bar (layout/BottomNav): five tabs, the selected one in a
 * soft pill with its icon in the accent. The pill is drawn wherever `pill`
 * says, so a scene can glide it with a spring the way the real one glides.
 *
 * Memoised, and every prop but `strings` is a plain number, so outside a
 * glide or a press nothing changes and the bar and its five icons are left
 * alone.
 */
export const TabBar = memo(function TabBar({
  strings,
  pill,
  selected,
  leaving,
  change = 1,
  selectedScale = 1,
  pressedIndex,
  pressedScale = 1,
  away = 0,
}: TabBarProps) {
  const slot = SCREEN.width / TABS.length;
  return (
    <div
      className="absolute inset-x-0 flex border-t border-[var(--border)] bg-bg"
      style={{
        top: TAB_BAR.top,
        height: TAB_BAR.height,
        opacity: 1 - away,
        transform: away > 0 ? `translateY(${away * TAB_BAR.height}px)` : undefined,
      }}
    >
      <span
        className="absolute h-[30px] w-14 rounded-full bg-fill-active"
        style={{ left: slot * pill + slot / 2 - 28, top: TAB_BAR.height / 2 - 25 }}
      />
      {TABS.map((tab, index) => {
        const Icon = tab.icon;
        const lit = index === selected ? change : index === leaving ? 1 - change : 0;
        const scale = (index === selected ? selectedScale : 1) * (index === pressedIndex ? pressedScale : 1);
        return (
          <div
            key={tab.key}
            className="relative flex min-w-0 flex-1 flex-col items-center justify-center gap-1 text-[12px] font-medium"
          >
            <span className="relative flex h-[30px] items-center" style={{ transform: `scale(${scale})` }}>
              <Icon size={21} style={{ color: blend('var(--accent)', 'var(--text-secondary)', lit) }} />
            </span>
            <span style={{ color: blend('var(--text-primary)', 'var(--text-secondary)', lit) }}>
              {strings[tab.key]}
            </span>
          </div>
        );
      })}
    </div>
  );
});

/** The system's gesture strip at the foot of the screen, with its handle. */
export function GestureStrip() {
  return (
    <div
      className="absolute inset-x-0 flex items-center justify-center bg-bg"
      style={{ top: GESTURE.top, height: GESTURE.height }}
    >
      <span className="h-[4px] w-[108px] rounded-full bg-fg opacity-40" />
    </div>
  );
}

/**
 * The app around its content: the content, the tab bar, the gesture strip.
 * `children` are drawn in the screen's own u, so a screen places itself with
 * the storyboard's numbers as they are.
 */
export function AppChrome({ children, tabs }: { children: ReactNode; tabs: ReactNode }) {
  return (
    <div className="absolute inset-0 overflow-hidden bg-bg">
      {children}
      {tabs}
      <GestureStrip />
    </div>
  );
}

// -- pictures -------------------------------------------------------------

/**
 * What stands in for a video's thumbnail: a small flat landscape. A picture,
 * so it reads as something someone filmed; flat, so it sits with the rest.
 * `mirrored` gives a second video its own picture.
 */
export function Landscape({ style, mirrored = false }: { style?: CSSProperties; mirrored?: boolean }) {
  return (
    <svg
      viewBox="0 0 160 90"
      preserveAspectRatio="xMidYMid slice"
      style={{ display: 'block', transform: mirrored ? 'scaleX(-1)' : undefined, ...style }}
      aria-hidden="true"
    >
      <rect width="160" height="90" fill="#9fc3d8" />
      <circle cx="118" cy="30" r="11" fill="#f6e3b0" />
      <path d="M0 62 C 28 44 52 46 78 58 S 128 48 160 56 V90 H0Z" fill="#6d9a74" />
      <path d="M0 74 C 36 60 70 64 104 74 S 146 70 160 72 V90 H0Z" fill="#4f7a58" />
      <path d="M22 70 l6 -18 l6 18z M34 72 l5 -14 l5 14z" fill="#2f5a3c" />
    </svg>
  );
}

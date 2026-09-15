import type { CSSProperties } from 'react';

import { cn } from '@/lib/cn';
import { platformPresentation } from '@/lib/platforms';
import type { PlatformId } from '@/types';

interface PlatformBadgeProps {
  platform: PlatformId;
  size?: 'sm' | 'md' | 'lg';
  className?: string;
}

const SIZES = {
  sm: 'size-5 text-[9px] rounded-[5px]',
  md: 'size-7 text-[10.5px] rounded-[7px]',
  lg: 'size-9 text-[12px] rounded-[9px]',
} as const;

/** The logo takes the room the two-letter monogram had inside the tile. */
const ICON_SIZES = {
  sm: 'size-3',
  md: 'size-4',
  lg: 'size-5',
} as const;

/**
 * A tinted tile carrying the platform's logo, in the platform's colour. Links
 * that have no platform behind them -- a direct file, a plain web page -- keep
 * a monogram in the same tile, so every source still reads at a glance and
 * every badge has the same footprint.
 */
export function PlatformBadge({ platform, size = 'md', className }: PlatformBadgeProps) {
  const { color, lightColor, icon, monogram, label } = platformPresentation(platform);

  // The light theme may need a deeper shade than the brand colour to stay
  // legible; the dark theme uses the brand colour as it is.
  const style = {
    '--badge-light': lightColor ?? color,
    '--badge-dark': color,
    color: 'var(--badge)',
    backgroundColor: 'color-mix(in srgb, var(--badge) 16%, transparent)',
    boxShadow: 'inset 0 0 0 1px color-mix(in srgb, var(--badge) 28%, transparent)',
  } as CSSProperties;

  return (
    <span
      title={label}
      aria-label={label}
      role="img"
      style={style}
      className={cn(
        'inline-flex shrink-0 items-center justify-center font-bold uppercase tracking-tight',
        '[--badge:var(--badge-light)] dark:[--badge:var(--badge-dark)]',
        SIZES[size],
        className,
      )}
    >
      {icon ? (
        <svg viewBox="0 0 24 24" aria-hidden="true" className={cn('fill-current', ICON_SIZES[size])}>
          <path d={icon} />
        </svg>
      ) : (
        monogram
      )}
    </span>
  );
}

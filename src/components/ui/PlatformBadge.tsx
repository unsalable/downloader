import { File, Globe, Link, type LucideIcon } from 'lucide-react';

import { cn } from '@/lib/cn';
import { platformPresentation } from '@/lib/platforms';
import type { PlatformId } from '@/types';

interface PlatformBadgeProps {
  platform: PlatformId;
  size?: 'sm' | 'md' | 'lg';
  className?: string;
}

/** Tile, corner and logo, in pixels. The corner is about 24% of the tile, which
 *  is the proportion of a home-screen icon; the logo takes about 58% of it. */
const SIZES = {
  sm: { tile: 'size-[22px] rounded-[5px]', glyph: 13 },
  md: { tile: 'size-7 rounded-[7px]', glyph: 16 },
  lg: { tile: 'size-9 rounded-[9px]', glyph: 21 },
} as const;

/** What stands in for a logo where there is no platform to have one. */
const NEUTRAL_ICON: Partial<Record<PlatformId, LucideIcon>> = {
  direct: File,
  generic: Globe,
  unknown: Link,
};

/**
 * The platform as an app icon: a rounded square in the brand's own colour with
 * the logo in white on it, at full strength in both themes. A link with no
 * platform behind it -- a direct file, a plain web page -- gets a neutral tile
 * of the same footprint, so a column of badges stays aligned.
 */
export function PlatformBadge({ platform, size = 'md', className }: PlatformBadgeProps) {
  const { tile, glyph, ring, icon, label } = platformPresentation(platform);
  const { tile: tileClass, glyph: glyphSize } = SIZES[size];
  const Neutral = NEUTRAL_ICON[platform] ?? Link;

  return (
    <span
      title={label}
      aria-label={label}
      role="img"
      style={
        tile
          ? {
              background: tile,
              color: glyph ?? 'white',
              boxShadow: ring ? 'inset 0 0 0 1px rgb(255 255 255 / 0.14)' : undefined,
            }
          : undefined
      }
      className={cn(
        'inline-flex shrink-0 items-center justify-center',
        !tile && 'bg-surface-active text-fg-muted',
        tileClass,
        className,
      )}
    >
      {tile && icon ? (
        <svg
          viewBox="0 0 24 24"
          width={glyphSize}
          height={glyphSize}
          aria-hidden="true"
          className="block fill-current"
        >
          <path d={icon} />
        </svg>
      ) : (
        <Neutral size={glyphSize} strokeWidth={2} aria-hidden="true" />
      )}
    </span>
  );
}

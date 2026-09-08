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

/**
 * A tinted monogram tile rather than the platform's own logo -- it keeps the
 * source legible at a glance without shipping anyone's trademarked artwork,
 * and it stays visually consistent across every provider.
 */
export function PlatformBadge({ platform, size = 'md', className }: PlatformBadgeProps) {
  const { color, monogram, label } = platformPresentation(platform);

  return (
    <span
      title={label}
      aria-label={label}
      style={{
        color,
        backgroundColor: `color-mix(in srgb, ${color} 16%, transparent)`,
        boxShadow: `inset 0 0 0 1px color-mix(in srgb, ${color} 28%, transparent)`,
      }}
      className={cn(
        'inline-flex shrink-0 items-center justify-center font-bold uppercase tracking-tight',
        SIZES[size],
        className,
      )}
    >
      {monogram}
    </span>
  );
}

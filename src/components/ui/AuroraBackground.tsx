import type { CSSProperties, HTMLAttributes, ReactNode } from 'react';

import { cn } from '@/lib/cn';

interface AuroraBackgroundProps extends HTMLAttributes<HTMLDivElement> {
  children?: ReactNode;
  /** Fade the light out toward the bottom instead of filling the frame. */
  showRadialGradient?: boolean;
}

/**
 * Slow bands of colour behind the content.
 *
 * Two copies of the same repeating gradient are stacked and offset: one holds
 * still, the other drifts across it, and where they cross the bands brighten.
 * That is the whole effect -- a `background-position` animation the compositor
 * owns, so an idle window costs no main-thread work and no canvas.
 *
 * The palette comes from the theme's three aurora stops rather than a fixed
 * blue-violet, so the light stays in the same family as the rest of the app.
 */
export function AuroraBackground({
  className,
  children,
  showRadialGradient = true,
  ...rest
}: AuroraBackgroundProps) {
  // The veil is painted in the page background colour, so the bands read as
  // gaps of light rather than a wash of colour over the whole surface.
  const layer: CSSProperties = {
    backgroundImage: `
      repeating-linear-gradient(100deg, var(--bg) 0%, var(--bg) 9%, transparent 13%, transparent 19%, var(--bg) 26%),
      repeating-linear-gradient(100deg, var(--aurora-1) 6%, var(--aurora-2) 16%, var(--aurora-3) 26%, var(--aurora-2) 36%, var(--aurora-1) 46%)
    `,
    backgroundSize: '300% 200%, 200% 180%',
    backgroundPosition: '50% 50%, 50% 50%',
    opacity: 'var(--aurora-opacity)',
  };

  return (
    <div className={cn('relative', className)} {...rest}>
      <div
        aria-hidden="true"
        className={cn(
          'pointer-events-none absolute inset-0 overflow-hidden',
          showRadialGradient &&
            '[mask-image:radial-gradient(120%_85%_at_50%_-10%,black_25%,transparent_75%)]',
        )}
      >
        <div
          style={layer}
          className={cn(
            'absolute -inset-[18%] blur-[46px] will-change-transform',
            // The moving copy. Difference blending against the still one is what
            // makes the bands separate instead of averaging into a smear.
            'after:absolute after:inset-0 after:content-[""]',
            'after:[background-image:inherit] after:[background-size:220%_140%]',
            'after:mix-blend-difference after:animate-aurora',
          )}
        />
      </div>

      {children}
    </div>
  );
}

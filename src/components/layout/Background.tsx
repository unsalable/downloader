import { AuroraBackground } from '@/components/ui/AuroraBackground';
import { IS_MOBILE } from '@/lib/platform';

/**
 * Ambient backdrop for the Home screen.
 *
 * The aurora does the colour; a fine noise layer keeps the large flat areas
 * from banding on 8-bit panels, and a bottom fade hands the page back to the
 * flat background before any text sits on it. No canvas, no WebGL, no
 * requestAnimationFrame -- the movement is a background-position animation the
 * compositor runs off the main thread.
 */
export function Background({ active }: { active: boolean }) {
  if (!active) return null;

  // On a phone the moving aurora is a large blurred layer repainted every
  // frame, which kept the GPU busy -- and the phone warm -- for as long as Home
  // was open. The same light, standing still, costs one paint.
  if (IS_MOBILE) {
    return (
      <div
        aria-hidden="true"
        className="pointer-events-none absolute inset-x-0 top-0 h-[55%]"
        style={{
          opacity: 'var(--aurora-opacity)',
          backgroundImage: `
            radial-gradient(70% 55% at 20% 0%, var(--aurora-1), transparent 70%),
            radial-gradient(60% 50% at 85% 5%, var(--aurora-2), transparent 70%)
          `,
          maskImage: 'linear-gradient(to bottom, black 35%, transparent)',
          WebkitMaskImage: 'linear-gradient(to bottom, black 35%, transparent)',
        }}
      />
    );
  }

  return (
    <div aria-hidden="true" className="pointer-events-none absolute inset-0 overflow-hidden">
      <AuroraBackground className="absolute inset-x-0 top-0 h-[62%]" />

      {/* Inlined as an SVG data URI so it costs no request. */}
      <div
        className="absolute inset-0 opacity-[0.04] mix-blend-overlay dark:opacity-[0.06]"
        style={{
          backgroundImage:
            "url(\"data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' width='160' height='160'%3E%3Cfilter id='n'%3E%3CfeTurbulence type='fractalNoise' baseFrequency='0.9' numOctaves='3'/%3E%3C/filter%3E%3Crect width='160' height='160' filter='url(%23n)'/%3E%3C/svg%3E\")",
        }}
      />

      <div className="absolute inset-0 bg-[linear-gradient(to_bottom,transparent_0%,var(--bg)_72%)]" />
    </div>
  );
}

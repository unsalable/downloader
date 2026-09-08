import { AuroraBackground } from '@/components/ui/AuroraBackground';

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

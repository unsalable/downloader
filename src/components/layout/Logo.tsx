/**
 * The application mark: a six-blade aperture.
 *
 * Same geometry as the generated app icon (`scripts/generate_icon.py`), drawn
 * here as vectors so it stays crisp at any size and follows the theme.
 */
export function Logo({ size = 24, className }: { size?: number; className?: string }) {
  const gradientId = 'ud-logo-gradient';

  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 48 48"
      fill="none"
      className={className}
      aria-hidden="true"
    >
      <defs>
        <linearGradient id={gradientId} x1="4" y1="4" x2="44" y2="44" gradientUnits="userSpaceOnUse">
          <stop stopColor="var(--accent)" />
          <stop offset="1" stopColor="var(--accent-hover)" />
        </linearGradient>
        <mask id="ud-logo-mask">
          <rect width="48" height="48" fill="black" />
          <circle cx="24" cy="24" r="20" fill="white" />
          {/* Hexagonal opening. */}
          <path d="M24 10.6 35.6 17.3 35.6 30.7 24 37.4 12.4 30.7 12.4 17.3Z" fill="black" />
          {/* Blade seams. Each runs outward from an opening vertex along that
              edge's direction -- the tangential offset is what reads as an iris
              rather than a plain segmented ring. */}
          <g stroke="black" strokeWidth="2.6" strokeLinecap="round">
            <path d="M24 10.6 50 25.6" />
            <path d="M35.6 17.3 35.6 47.3" />
            <path d="M35.6 30.7 9.6 45.7" />
            <path d="M24 37.4 -2 22.4" />
            <path d="M12.4 30.7 12.4 0.7" />
            <path d="M12.4 17.3 38.4 2.3" />
          </g>
        </mask>
      </defs>
      <circle cx="24" cy="24" r="20" fill={`url(#${gradientId})`} mask="url(#ud-logo-mask)" />
    </svg>
  );
}

import { useId, type CSSProperties, type ReactNode } from 'react';
import { AbsoluteFill, Easing, interpolate, spring, useCurrentFrame, useVideoConfig } from 'remotion';

import { platformPresentation } from '@/lib/platforms';
import type { PlatformId } from '@/types';
import { INTRO_FPS } from './scene';

/*
 * The film's small vocabulary, shared by every scene so the whole of it moves
 * and looks like one thing.
 *
 * Everything here is a pure function of the frame. Remotion draws a frame by
 * rendering the tree for it, so a CSS transition or a Motion animation would
 * run on its own clock and be caught half-way in a render; nothing in the film
 * may use either.
 *
 * Units. `d` is design px on the stage: the film is laid out for a 390x844
 * phone with (0,0) at the middle of the screen, and the stage is scaled by
 * s = min(W/390, H/844) to whatever phone it plays on, so the same numbers
 * work from 360 to 430 wide. `u` is px on the screen of the phone drawn in the
 * film -- 390x844, origin at its top-left -- which the camera maps onto the
 * stage.
 */

// -- time -----------------------------------------------------------------

/**
 * The app's curves (lib/motion.ts), as Remotion easings. `out` is the
 * signature -- things arriving and settling. `inOut` is the camera's, which
 * has to leave as gently as it arrives. `in` is for things leaving.
 */
export const EASE = {
  out: Easing.bezier(0.22, 1, 0.36, 1),
  inOut: Easing.bezier(0.65, 0, 0.35, 1),
  in: Easing.bezier(0.3, 0, 1, 1),
} as const;

/**
 * The app's springs (lib/motion.ts), as Remotion spring configs, and one of
 * the film's own: `stage`, for the phone arriving with some weight.
 */
export const SPRING = {
  snap: { stiffness: 620, damping: 40, mass: 0.6 },
  glide: { stiffness: 480, damping: 40, mass: 0.75 },
  settle: { stiffness: 420, damping: 34, mass: 0.85 },
  stage: { stiffness: 140, damping: 22, mass: 1 },
} as const;

/** 0 before `from`, 1 after `from + duration`, eased in between. */
export function ramp(frame: number, from: number, duration: number, easing = EASE.out): number {
  if (duration <= 0) return frame >= from ? 1 : 0;
  return interpolate(frame, [from, from + duration], [0, 1], {
    easing,
    extrapolateLeft: 'clamp',
    extrapolateRight: 'clamp',
  });
}

/** A spring from 0 toward 1 that starts at `from`; 0 before it. */
export function springFrom(frame: number, from: number, config: (typeof SPRING)[keyof typeof SPRING]): number {
  if (frame < from) return 0;
  return spring({ frame: frame - from, fps: INTRO_FPS, config });
}

/**
 * A value through a list of [frame, value] keys, eased segment by segment and
 * held at either end. A hold is two keys with the same value. What the camera
 * and anything with more than two stops are written in.
 */
export function track(
  frame: number,
  keys: readonly (readonly [number, number])[],
  easing = EASE.inOut,
): number {
  if (keys.length === 1) return keys[0]![1];
  return interpolate(
    frame,
    keys.map((key) => key[0]),
    keys.map((key) => key[1]),
    { easing, extrapolateLeft: 'clamp', extrapolateRight: 'clamp' },
  );
}

/**
 * `track` for a zoom, which is eased in log space: a push from 0.6 to 1.2
 * and one from 1.2 to 2.4 then feel like the same move, as they do through a
 * real lens.
 */
export function trackZoom(
  frame: number,
  keys: readonly (readonly [number, number])[],
  easing = EASE.inOut,
): number {
  return Math.exp(track(frame, keys.map(([at, value]) => [at, Math.log(value)] as const), easing));
}

export function mix(from: number, to: number, amount: number): number {
  return from + (to - from) * amount;
}

/**
 * A colour `amount` of the way from `off` to `on`, for a colour that changes
 * over frames instead of in one. Theme tokens stay tokens; the WebView mixes.
 */
export function blend(on: string, off: string, amount: number): string {
  if (amount >= 1) return on;
  if (amount <= 0) return off;
  return `color-mix(in srgb, ${on} ${(amount * 100).toFixed(1)}%, ${off})`;
}

/**
 * A control being pressed at `at`: it gives to `depth` over four frames and
 * springs back (SNAP), the way the app's own controls answer a finger. A
 * scale factor; 1 when nothing is happening.
 */
export function press(frame: number, at: number, depth = 0.96): number {
  if (frame < at) return 1;
  if (frame < at + 4) return mix(1, depth, ramp(frame, at, 4));
  return mix(depth, 1, springFrom(frame, at + 4, SPRING.snap));
}

/**
 * The composition's frame, whichever sequence a scene is mounted in. Scenes
 * are written in the film's own frame numbers -- the storyboard's -- and are
 * mounted at `from`, where Remotion restarts their clock at 0.
 */
export function useFilmFrame(from: number): number {
  return useCurrentFrame() + from;
}

// -- the stage ------------------------------------------------------------

/** The stage's scale and its size in design px. */
export function useStage() {
  const { width, height } = useVideoConfig();
  const s = Math.min(width / 390, height / 844);
  return { s, width: width / s, height: height / s };
}

/**
 * The stage: an origin at the middle of the screen, scaled so 390x844 design
 * px fit the phone the film plays on. Everything inside is placed in d from
 * that middle, with `At` or with its own transform.
 */
export function Stage({ children, style }: { children: ReactNode; style?: CSSProperties }) {
  const { s } = useStage();
  return (
    <AbsoluteFill style={{ overflow: 'hidden', ...style }}>
      <div style={{ position: 'absolute', left: '50%', top: '50%', width: 0, height: 0, transform: `scale(${s})` }}>
        {children}
      </div>
    </AbsoluteFill>
  );
}

/** Something centred on (x, y) of the stage, in d. */
export function At({ x = 0, y = 0, children, style }: { x?: number; y?: number; children: ReactNode; style?: CSSProperties }) {
  return (
    <div
      style={{
        position: 'absolute',
        left: x,
        top: y,
        transform: 'translate(-50%, -50%)',
        whiteSpace: 'nowrap',
        ...style,
      }}
    >
      {children}
    </div>
  );
}

// -- type -----------------------------------------------------------------

interface TypeInProps {
  text: string;
  /** Frame the first character starts to arrive. */
  start?: number;
  /** Frames between one character and the next. */
  stagger?: number;
  /** Frames each character takes to come into focus. */
  settle?: number;
  /** The frame to read; the scene's own clock unless given. */
  frame?: number;
  className?: string;
  style?: CSSProperties;
}

/**
 * A line that arrives a character at a time, each coming out of a soft blur
 * into focus -- the opening move of the film the user chose as the model.
 *
 * The whole line is laid out from the first frame, so it never shifts; each
 * character appears where it will stay. Only the handful in the middle of
 * arriving carry a blur, which keeps the filter work on a phone to a few
 * glyphs a frame. Words are kept whole, so the line still wraps between them.
 */
export function TypeIn({ text, start = 0, stagger = 1.4, settle = 12, frame: given, className, style }: TypeInProps) {
  const own = useCurrentFrame();
  const frame = given ?? own;
  let index = 0;

  return (
    <span className={className} style={style}>
      {text.split(/(\s+)/).map((part, partIndex) => {
        if (/^\s+$/.test(part)) {
          index += part.length;
          return part;
        }
        return (
          <span key={partIndex} style={{ display: 'inline-block', whiteSpace: 'nowrap' }}>
            {Array.from(part).map((char, charIndex) => {
              const t = ramp(frame, start + index++ * stagger, settle);
              return (
                <span
                  key={charIndex}
                  style={{
                    display: 'inline-block',
                    opacity: t,
                    // Dropped once it is under a fiftieth of a pixel, not at
                    // exactly 1, so the tail of each settle costs no filter.
                    filter: t > 0 && t < 0.98 ? `blur(${(1 - t) * 5}px)` : undefined,
                    transform: t < 1 ? `translateY(${(1 - t) * 0.18}em)` : undefined,
                  }}
                >
                  {char}
                </span>
              );
            })}
          </span>
        );
      })}
    </span>
  );
}

/**
 * A caption: a few words over the film, each resolving out of a blur in turn,
 * leaving as one. Set in the band at the top of the stage, which the camera
 * keeps clear whenever a caption is up. Place it directly inside `Stage`.
 *
 * Each word takes 12 frames, four after the one before, so no more than three
 * are blurred at once.
 */
export function Caption({ text, frame, start, exit }: { text: string; frame: number; start: number; exit?: number }) {
  const { height } = useStage();
  const gone = exit == null ? 0 : ramp(frame, exit, 8, EASE.in);
  if (frame < start || gone >= 1) return null;
  const words = text.split(/\s+/);
  return (
    <div
      style={{
        position: 'absolute',
        left: 0,
        top: -height / 2 + 104,
        width: 342,
        transform: 'translate(-50%, -50%)',
        textAlign: 'center',
        textWrap: 'balance',
        fontSize: 19,
        fontWeight: 600,
        letterSpacing: '-0.015em',
        color: 'var(--text-primary)',
        opacity: 1 - gone,
        filter: gone > 0 ? `blur(${gone * 4}px)` : undefined,
      }}
    >
      {words.map((word, index) => {
        const at = start + index * 4;
        const shown = ramp(frame, at, 10);
        const sharp = ramp(frame, at, 12);
        return (
          <span key={index}>
            {index > 0 && ' '}
            <span
              style={{
                display: 'inline-block',
                opacity: shown,
                filter: sharp < 1 ? `blur(${(1 - sharp) * 6}px)` : undefined,
                transform: sharp < 1 ? `translateY(${(1 - sharp) * 4}px)` : undefined,
              }}
            >
              {word}
            </span>
          </span>
        );
      })}
    </div>
  );
}

// -- geometry -------------------------------------------------------------

interface DashedCircleProps {
  /** Diameter, in px. */
  size: number;
  /** Degrees the dashes have travelled round, for a ring that is slowly alive. */
  turn?: number;
  stroke?: string;
  strokeWidth?: number;
  dash?: number;
  gap?: number;
  opacity?: number;
  style?: CSSProperties;
}

/**
 * A thin dashed ring -- the reference's frame for a single word. Hairline and
 * quiet: it holds the word, it is not the subject. `dash` and `gap` may move,
 * so a ring can close: keep their sum constant and the pattern grows solid
 * without jumping.
 *
 * No mask: a ring drawn on would need one, and a mask is an offscreen layer
 * composited every frame the ring moves -- 60% of the scene's raster time,
 * measured, for an effect the film does not use.
 */
export function DashedCircle({
  size,
  turn = 0,
  stroke = 'var(--text-tertiary)',
  strokeWidth = 1.25,
  dash = 7,
  gap = 8,
  opacity = 0.5,
  style,
}: DashedCircleProps) {
  const r = (size - strokeWidth) / 2;
  return (
    <svg
      width={size}
      height={size}
      viewBox={`0 0 ${size} ${size}`}
      style={{ overflow: 'visible', opacity, ...style }}
      aria-hidden="true"
    >
      <circle
        cx={size / 2}
        cy={size / 2}
        r={r}
        fill="none"
        stroke={stroke}
        strokeWidth={strokeWidth}
        strokeDasharray={gap <= 0.01 ? undefined : `${dash} ${gap}`}
        transform={`rotate(${turn - 90} ${size / 2} ${size / 2})`}
      />
    </svg>
  );
}

/** A dashed horizontal rule, drawn on from its left end. */
export function DashedLine({
  width,
  drawn = 1,
  stroke = 'var(--text-tertiary)',
  dash = 7,
  gap = 8,
  opacity = 0.5,
  style,
}: {
  width: number;
  drawn?: number;
  stroke?: string;
  dash?: number;
  gap?: number;
  opacity?: number;
  style?: CSSProperties;
}) {
  return (
    <svg width={width} height={2} style={{ overflow: 'visible', opacity, ...style }} aria-hidden="true">
      <line
        x1={0}
        y1={1}
        x2={width * Math.min(1, Math.max(0, drawn))}
        y2={1}
        stroke={stroke}
        strokeWidth={1}
        strokeDasharray={`${dash} ${gap}`}
      />
    </svg>
  );
}

// -- the phone ------------------------------------------------------------

/** The phone's screen in u, the app's own CSS px. */
export const SCREEN = { width: 390, height: 844 } as const;
const BEZEL = 8;

/**
 * A phone at its real size: a 390x844 screen in u, origin at the screen's
 * top-left, in a thin body. Put it in a `PhoneCamera` to place it on the
 * stage. No shadow: a zoom would re-rasterise a large blur every frame.
 */
export function PhoneFrame({ children }: { children: ReactNode }) {
  return (
    <div
      style={{
        position: 'absolute',
        left: -BEZEL,
        top: -BEZEL,
        width: SCREEN.width + BEZEL * 2,
        height: SCREEN.height + BEZEL * 2,
        borderRadius: 60,
        background: 'var(--surface-sunken)',
        boxShadow: 'inset 0 0 0 1.5px var(--border-strong)',
      }}
    >
      {/* Side keys */}
      <span style={{ position: 'absolute', right: -3, top: 170, width: 3, height: 44, borderRadius: 2, background: 'var(--border-strong)' }} />
      <span style={{ position: 'absolute', right: -3, top: 236, width: 3, height: 72, borderRadius: 2, background: 'var(--border-strong)' }} />
      <div
        style={{
          position: 'absolute',
          left: BEZEL,
          top: BEZEL,
          width: SCREEN.width,
          height: SCREEN.height,
          borderRadius: 52,
          overflow: 'hidden',
          background: 'var(--bg)',
        }}
      >
        {children}
        {/* The camera's punch-hole */}
        <span
          style={{
            position: 'absolute',
            left: 195 - 5,
            top: 20 - 5,
            width: 10,
            height: 10,
            borderRadius: 5,
            background: '#000',
            opacity: 0.85,
          }}
        />
      </div>
    </div>
  );
}

/**
 * The camera on a phone: puts the screen point (fx, fy), in u, at the stage's
 * middle -- 24 d below it, to leave the caption band free -- magnified `m`
 * times and rolled `roll` degrees. `turnY` turns the phone about its upright
 * axis, under the stage's perspective. Place it directly inside `Stage`.
 */
export function PhoneCamera({
  fx,
  fy,
  m,
  roll = 0,
  turnY = 0,
  children,
}: {
  fx: number;
  fy: number;
  m: number;
  roll?: number;
  turnY?: number;
  children: ReactNode;
}) {
  return (
    <div
      style={{
        position: 'absolute',
        left: 0,
        top: 0,
        transformOrigin: '0 0',
        transform: `translate(0px, 24px) rotate(${roll}deg) scale(${m}) translate(${-fx}px, ${-fy}px)`,
      }}
    >
      <div
        style={{
          position: 'absolute',
          left: 0,
          top: 0,
          transformOrigin: `${fx}px ${fy}px`,
          transform: turnY ? `perspective(1600px) rotateY(${turnY}deg)` : undefined,
        }}
      >
        {children}
      </div>
    </div>
  );
}

/**
 * Android's touch ripple: a disc spreading from the middle of the control
 * and fading, clipped by the control (give its parent `overflow: hidden` and
 * `position: relative`). Drawn in the text colour at 12%, or on an accent
 * button in the accent's ink at 18%.
 */
export function Ripple({
  frame,
  at,
  size,
  color = 'var(--text-primary)',
  strength = 0.12,
}: {
  frame: number;
  at: number;
  /** Diameter at its widest, in px. */
  size: number;
  color?: string;
  strength?: number;
}) {
  const t = ramp(frame, at, 12);
  if (frame < at || t >= 1) return null;
  return (
    <span
      style={{
        position: 'absolute',
        left: '50%',
        top: '50%',
        width: size,
        height: size,
        marginLeft: -size / 2,
        marginTop: -size / 2,
        borderRadius: '50%',
        background: color,
        opacity: strength * (1 - t),
        transform: `scale(${mix(0.4, 1, t)})`,
        pointerEvents: 'none',
      }}
    />
  );
}

/**
 * A segmented control (ui/Segmented), drawn from the frame: the thumb sits at
 * `thumb`, which may fall between two options while it glides. `width` is the
 * whole track.
 */
export function Segmented({
  options,
  thumb,
  width,
  height = 44,
  pressed = [],
}: {
  options: { label: string; icon?: ReactNode }[];
  thumb: number;
  width: number;
  height?: number;
  /** A press scale per option, from `press()`. */
  pressed?: number[];
}) {
  const inner = width - 4;
  const each = inner / options.length;
  return (
    <div
      style={{ position: 'relative', display: 'flex', width, height, padding: 2, borderRadius: 10, background: 'var(--fill)' }}
    >
      <span
        className="bg-surface shadow-[0_1px_2px_rgb(0_0_0/0.08)] dark:bg-fill-active dark:shadow-none"
        style={{ position: 'absolute', top: 2, bottom: 2, left: 2 + each * thumb, width: each, borderRadius: 8 }}
      />
      {options.map((option, index) => (
        <span
          key={index}
          className="relative flex items-center justify-center gap-1.5 text-[14px] font-medium"
          style={{
            width: each,
            // The label under the thumb is lit, and the two cross over as it
            // passes, as the real control's colour transition does.
            color: blend('var(--text-primary)', 'var(--text-secondary)', Math.max(0, 1 - Math.abs(thumb - index))),
            transform: `scale(${pressed[index] ?? 1})`,
          }}
        >
          {option.icon}
          {option.label}
        </span>
      ))}
    </div>
  );
}

/**
 * One value giving way to another at `at`: the old one lifts out over four
 * frames and the new one settles in over six, in the same place.
 */
export function Swap({ frame, at, from, to }: { frame: number; at: number; from: ReactNode; to: ReactNode }) {
  if (frame < at) return <>{from}</>;
  const out = ramp(frame, at, 4, EASE.in);
  const inn = ramp(frame, at + 2, 6);
  return (
    <span style={{ display: 'inline-grid' }}>
      {out < 1 && (
        <span style={{ gridArea: '1 / 1', opacity: 1 - out, transform: `translateY(${-3 * out}px)` }}>{from}</span>
      )}
      <span style={{ gridArea: '1 / 1', opacity: inn, transform: inn < 1 ? `translateY(${3 * (1 - inn)}px)` : undefined }}>
        {to}
      </span>
    </span>
  );
}

/** The app's spinner (ui/Spinner), turned by the frame instead of a CSS animation. */
export function Spinner({ frame, size = 14 }: { frame: number; size?: number }) {
  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 24 24"
      fill="none"
      aria-hidden="true"
      style={{ transform: `rotate(${frame * 12}deg)`, flexShrink: 0 }}
    >
      <circle cx="12" cy="12" r="9" stroke="currentColor" strokeOpacity="0.22" strokeWidth="2.5" />
      <path d="M21 12a9 9 0 0 0-9-9" stroke="currentColor" strokeWidth="2.5" strokeLinecap="round" />
    </svg>
  );
}

// -- the app's own pieces -------------------------------------------------

/**
 * A platform as the app draws it (ui/PlatformBadge): an app-icon tile in the
 * brand's own colour, the logo in white, at full strength in both themes.
 */
export function PlatformTile({ platform, size, style }: { platform: PlatformId; size: number; style?: CSSProperties }) {
  const { tile, glyph, ring, icon } = platformPresentation(platform);
  return (
    <div
      style={{
        width: size,
        height: size,
        borderRadius: size * 0.24,
        background: tile ?? 'var(--surface-active)',
        color: glyph ?? 'white',
        display: 'flex',
        alignItems: 'center',
        justifyContent: 'center',
        boxShadow: ring ? 'inset 0 0 0 1px rgb(255 255 255 / 0.14)' : undefined,
        flexShrink: 0,
        ...style,
      }}
    >
      {icon && (
        <svg viewBox="0 0 24 24" width={size * 0.58} height={size * 0.58} fill="currentColor" aria-hidden="true">
          <path d={icon} />
        </svg>
      )}
    </div>
  );
}

/**
 * The mark (layout/Logo) with its iris free to move: `open` takes the opening
 * from a pinhole to its real size while the blades turn into place, so the
 * aperture opens the way a lens does. At 1 it is exactly the app's mark.
 * `from` and `to` colour the disc: the theme's accent unless given, which the
 * launcher icon does, since a launcher does not follow the app's theme.
 */
export function MarkIris({
  size,
  open = 1,
  from = 'var(--accent)',
  to = 'var(--accent-hover)',
}: {
  size: number;
  open?: number;
  from?: string;
  to?: string;
}) {
  const id = `intro-iris-${useId().replace(/:/g, '')}`;
  const hole = mix(0.12, 1, open);
  const turn = mix(-50, 0, open);
  // Only the opening shrinks. Each seam still starts at its corner of the
  // opening and runs on, along that edge, past the rim -- so a nearly closed
  // iris is six blades meeting at a pinhole, not a small star floating in a
  // disc.
  const corners = HEXAGON.map(([x, y]) => [24 + (x - 24) * hole, 24 + (y - 24) * hole] as const);
  return (
    <svg width={size} height={size} viewBox="0 0 48 48" fill="none" aria-hidden="true">
      <defs>
        <linearGradient id={`${id}-g`} x1="4" y1="4" x2="44" y2="44" gradientUnits="userSpaceOnUse">
          <stop stopColor={from} />
          <stop offset="1" stopColor={to} />
        </linearGradient>
        <mask id={`${id}-m`}>
          <rect width="48" height="48" fill="black" />
          <circle cx="24" cy="24" r="20" fill="white" />
          <g transform={`rotate(${turn} 24 24)`}>
            <path d={`M${corners.map(([x, y]) => `${x} ${y}`).join(' ')}Z`} fill="black" />
            <g stroke="black" strokeWidth="3.4" strokeLinecap="round">
              {corners.map(([x, y], index) => {
                const [dx, dy] = SEAMS[index]!;
                return <path key={index} d={`M${x} ${y} ${x + dx} ${y + dy}`} />;
              })}
            </g>
          </g>
        </mask>
      </defs>
      <circle cx="24" cy="24" r="20" fill={`url(#${id}-g)`} mask={`url(#${id}-m)`} />
    </svg>
  );
}

/** The opening's corners and the seams leaving them, as layout/Logo draws them. */
const HEXAGON = [
  [24, 10.6],
  [35.6, 17.3],
  [35.6, 30.7],
  [24, 37.4],
  [12.4, 30.7],
  [12.4, 17.3],
] as const;
const SEAMS = [
  [26, 15],
  [0, 30],
  [-26, 15],
  [-26, -15],
  [0, -30],
  [26, -15],
] as const;

/**
 * The app's launcher icon as Android draws it: the mark on its adaptive
 * background (res/values/ic_launcher_background.xml), in the launcher's fixed
 * colours -- the same in both themes, like a brand tile. Anything drawn by the
 * system round the icon takes the same colours, not the app's theme.
 */
export const LAUNCHER = { from: '#ff8a4c', to: '#ff9d69' } as const;

export function LauncherIcon({ size }: { size: number }) {
  return (
    <span
      style={{
        width: size,
        height: size,
        borderRadius: '50%',
        background: '#1D1B23',
        display: 'flex',
        alignItems: 'center',
        justifyContent: 'center',
        flexShrink: 0,
      }}
    >
      <MarkIris size={size * 0.6} from={LAUNCHER.from} to={LAUNCHER.to} />
    </span>
  );
}

import { Link } from 'lucide-react';
import { memo, type CSSProperties } from 'react';

import {
  Caption,
  EASE,
  LAUNCHER,
  LauncherIcon,
  PhoneCamera,
  PhoneFrame,
  Stage,
  mix,
  press,
  ramp,
  track,
  trackZoom,
  useFilmFrame,
  useStage,
} from '../kit';
import type { IntroScene } from '../scene';
import { GestureStrip } from '../screens';
import type { IntroStrings } from '../strings';

/*
 * S2, the share sheet: another app, a link in it, and Android's share sheet
 * already up with the app among its targets. The camera starts tight on the
 * phone's bottom-left corner while a ring draws itself round the app's icon,
 * pulls back to the whole phone, the icon is tapped, and the app opens -- a
 * disc of the app's own ground spreading from behind the icon until it is all
 * there is, which the next scene starts on.
 *
 * The other app is shapes only and the other three targets are blank discs:
 * nothing in it may pass for a real app, and nothing may compete with ours.
 */

const FROM = 56;
const DURATION = 92;

/** Our share target: its centre on the phone's screen, in u, and its size. */
const OURS = { x: 60, y: 700, size: 56 } as const;

/** The shot the camera ends on: the whole phone (the storyboard's W). */
const WIDE = { fx: 195, fy: 422, m: 0.64 } as const;

/**
 * A slow push on the corner, a breath, then the pull back to the whole
 * phone. Every key is one frame of the film.
 */
const CAMERA = {
  fx: [[56, 72], [84, 70], [86, 70], [108, WIDE.fx]],
  fy: [[56, 706], [84, 702], [86, 702], [108, WIDE.fy]],
  m: [[56, 2.3], [84, 2.4], [86, 2.4], [108, WIDE.m]],
  roll: [[56, -4], [84, -2], [86, -2], [108, 0]],
} as const;

const RING = { from: 64, drawn: 20, fade: 130, radius: 34 } as const;

/**
 * The tap, pressed harder than the app's own controls are (0.96 and a 12%
 * ripple): it lands on the wide shot, where the icon is 36 d across and a
 * control's press would pass unseen.
 */
const TAP = { at: 130, depth: 0.88, ripple: 0.2 } as const;

/**
 * The app opening starts under the press, three frames in, and runs to the
 * scene's last frame. The icon is kept over it from its first frame, and
 * grows a little and fades over f135-141 (SPLASH).
 */
const FLOOD = { from: 133, frames: 14 } as const;
const SPLASH = { from: 135, frames: 6, scale: 1.1 } as const;

/**
 * The caption waits for the pull-back to carry the phone's top edge out of
 * its band (f102): TR is sharp by f122, EN by f126. It stays a few frames
 * into the app opening, so the longer EN line is whole for some ten frames
 * too, and is gone by f144, leaving the ground empty before the values.
 */
const CAPTION = { start: 102, exit: 136 } as const;

/** Our icon, the same at every frame; only the boxes round it move. */
const OurIcon = memo(function OurIcon() {
  return <LauncherIcon size={OURS.size} />;
});

/** The other app, under the sheet: a picture and a few lines of text, as shapes. */
const OtherApp = memo(function OtherApp() {
  return (
    <>
      <span className="absolute inset-x-0 bg-fill-active" style={{ top: 44, height: 219 }} />
      <span className="absolute rounded-[6px] bg-fill" style={{ left: 24, top: 283, width: 280, height: 12 }} />
      <span className="absolute rounded-[6px] bg-fill" style={{ left: 24, top: 305, width: 180, height: 12 }} />
      <span className="absolute rounded-full bg-fill" style={{ left: 24, top: 335, width: 32, height: 32 }} />
      <span className="absolute rounded-full bg-fill" style={{ left: 68, top: 346, width: 120, height: 10 }} />
    </>
  );
});

/**
 * The share sheet as Android draws it, less our target, which moves: the
 * grabber, the row saying what is being shared, and three other targets as
 * blank discs with a bar where their names would be.
 */
const Sheet = memo(function Sheet({ label }: { label: string }) {
  return (
    <div className="absolute inset-x-0 rounded-t-[28px] bg-surface" style={{ top: 560, bottom: 0 }}>
      <span className="absolute rounded-full bg-border-strong" style={{ left: 177, top: 12, width: 36, height: 4 }} />

      <span
        className="absolute flex items-center justify-center rounded-[10px] bg-fill text-fg-muted"
        style={{ left: 24, top: 32, width: 40, height: 40 }}
      >
        <Link size={18} />
      </span>
      <span className="absolute rounded-full bg-fill-hover" style={{ left: 76, top: 42, width: 120, height: 8 }} />
      <span className="absolute rounded-full bg-fill-hover" style={{ left: 76, top: 58, width: 76, height: 8 }} />

      {[150, 240, 330].map((x) => (
        <span key={x}>
          <span
            className="absolute rounded-full bg-fill-active"
            style={{ left: x - OURS.size / 2, top: OURS.y - 560 - OURS.size / 2, width: OURS.size, height: OURS.size }}
          />
          <span className="absolute rounded-full bg-fill-hover" style={{ left: x - 20, top: 182, width: 40, height: 6 }} />
        </span>
      ))}

      <span
        className="absolute text-center font-medium text-fg"
        style={{
          left: OURS.x - 40,
          top: 176,
          width: 80,
          fontSize: 11.5,
          lineHeight: '15px',
          textWrap: 'balance',
          overflow: 'hidden',
          maxHeight: 30,
        }}
      >
        {label}
      </span>
    </div>
  );
});

/**
 * The system's gesture strip over the sheet. Android's sheet runs on under
 * it, so here the strip takes the sheet's colour instead of the page's.
 */
const SheetGestureStrip = memo(function SheetGestureStrip() {
  return (
    <div className="contents" style={{ '--bg': 'var(--surface)' } as CSSProperties}>
      <GestureStrip />
    </div>
  );
});

/**
 * Our target, the one thing on the sheet that moves: the ring drawing round
 * it (the reference's "updating" ring), and the tap -- the icon giving under
 * the finger and a ripple spreading behind it, which is what an Android
 * launcher target does when pressed.
 */
function OurTarget({ f }: { f: number }) {
  const drawn = ramp(f, RING.from, RING.drawn, EASE.inOut);
  const ringFade = ramp(f, RING.fade, 5, EASE.in);
  const ripple = ramp(f, TAP.at, 12);
  const circumference = 2 * Math.PI * RING.radius;
  const box = RING.radius * 2 + 4;

  return (
    <>
      {f >= TAP.at && ripple < 1 && (
        <span
          className="absolute rounded-full bg-fg"
          style={{
            left: OURS.x - 46,
            top: OURS.y - 46,
            width: 92,
            height: 92,
            opacity: TAP.ripple * (1 - ripple),
            transform: `scale(${mix(28 / 46, 1, ripple)})`,
          }}
        />
      )}
      <div
        className="absolute"
        style={{
          left: OURS.x - OURS.size / 2,
          top: OURS.y - OURS.size / 2,
          width: OURS.size,
          height: OURS.size,
          transform: f >= TAP.at ? `scale(${press(f, TAP.at, TAP.depth)})` : undefined,
        }}
      >
        <OurIcon />
      </div>
      {/* Nothing is drawn until the ring starts: a round cap on an empty dash
          would still leave a dot at twelve o'clock. The ring is the share
          sheet's, round the launcher's icon -- system UI, which does not
          follow the app's theme -- so it takes the launcher's orange, not the
          accent the storyboard gave it: in light the accent is a rust, and
          would sit beside the mark as a second, different orange. */}
      {drawn > 0 && ringFade < 1 && (
        <svg
          className="absolute"
          width={box}
          height={box}
          viewBox={`0 0 ${box} ${box}`}
          style={{ left: OURS.x - box / 2, top: OURS.y - box / 2, opacity: 1 - ringFade }}
          aria-hidden="true"
        >
          <circle
            cx={box / 2}
            cy={box / 2}
            r={RING.radius}
            fill="none"
            stroke={LAUNCHER.from}
            strokeWidth={2}
            strokeLinecap="round"
            strokeDasharray={`${circumference * drawn} ${circumference}`}
            transform={`rotate(-90 ${box / 2} ${box / 2})`}
          />
        </svg>
      )}
    </>
  );
}

/**
 * The app opening, as Android 12 opens one: a disc of the app's ground
 * spreading from behind the tapped icon until it covers the stage, with the
 * icon kept on it for a moment, growing a little as it fades -- the launch
 * splash -- so the icon is never swapped for a blank disc. The disc is drawn
 * at its full size and scaled down, so it only ever moves by transform.
 *
 * It starts at the icon's footprint and its full size just reaches the
 * stage's farthest corner. On the in-out curve its edge leaves the icon
 * slowly enough to be seen growing, is past the phone's far corner on f142,
 * and spends the last frames on empty ground, a breath before the values.
 */
function Flood({ f, width, height }: { f: number; width: number; height: number }) {
  if (f < FLOOD.from) return null;
  // Where the icon is on the stage once the camera has settled on the wide shot.
  const x = WIDE.m * (OURS.x - WIDE.fx);
  const y = 24 + WIDE.m * (OURS.y - WIDE.fy);
  const size = 2 * (Math.hypot(width / 2 + Math.abs(x), height / 2 + Math.abs(y)) + 4);
  const grown = ramp(f, FLOOD.from, FLOOD.frames, EASE.inOut);
  const lifted = ramp(f, SPLASH.from, SPLASH.frames);
  const gone = ramp(f, SPLASH.from, SPLASH.frames, EASE.in);
  return (
    <>
      <div
        className="absolute rounded-full bg-bg"
        style={{
          left: x - size / 2,
          top: y - size / 2,
          width: size,
          height: size,
          transform: `scale(${mix((OURS.size * WIDE.m) / size, 1, grown)})`,
        }}
      />
      {/* The icon over the disc, where the camera shows it and still in the
          press, so the first frames match the one under it exactly. */}
      {gone < 1 && (
        <div
          className="absolute"
          style={{
            left: x - OURS.size / 2,
            top: y - OURS.size / 2,
            width: OURS.size,
            height: OURS.size,
            opacity: gone > 0 ? 1 - gone : undefined,
            transform: `scale(${WIDE.m * press(f, TAP.at, TAP.depth) * mix(1, SPLASH.scale, lifted)})`,
          }}
        >
          <OurIcon />
        </div>
      )}
    </>
  );
}

function Share({ strings }: { strings: IntroStrings }) {
  const f = useFilmFrame(FROM);
  const { width, height } = useStage();
  const shown = ramp(f, FROM, 8);

  return (
    <Stage>
      <div className="absolute left-0 top-0" style={{ opacity: shown < 1 ? shown : undefined }}>
        <PhoneCamera
          fx={track(f, CAMERA.fx)}
          fy={track(f, CAMERA.fy)}
          m={trackZoom(f, CAMERA.m)}
          roll={track(f, CAMERA.roll)}
        >
          <PhoneFrame>
            <OtherApp />
            <span className="absolute inset-0 bg-scrim" />
            <Sheet label={strings.appName} />
            <OurTarget f={f} />
            <SheetGestureStrip />
          </PhoneFrame>
        </PhoneCamera>
      </div>
      <Flood f={f} width={width} height={height} />
      {/* Over the flood, so its edge passes under the words, never through them. */}
      <Caption text={strings.share} frame={f} start={CAPTION.start} exit={CAPTION.exit} />
    </Stage>
  );
}

/*
 * Mounted eight frames early, at f48: the headline is sharp in both languages
 * by then (EN f46, TR f47) and holds until f54, so the mount's cost falls on
 * a still frame instead of on the last letters coming into focus.
 */
export const scene: IntroScene = { id: 'share', from: FROM, duration: DURATION, premountFor: 8, Component: Share };

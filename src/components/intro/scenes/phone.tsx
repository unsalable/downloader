import type { ReactNode } from 'react';

import {
  Caption,
  EASE,
  PhoneCamera,
  PhoneFrame,
  SPRING,
  Stage,
  mix,
  press,
  ramp,
  springFrom,
  track,
  trackZoom,
  useFilmFrame,
} from '../kit';
import type { IntroScene } from '../scene';
import { AppChrome, CONTENT, GESTURE, TabBar } from '../screens';
import { DownloadsScreen } from '../screens/downloads';
import { EditorPickScreen, EditorScreen } from '../screens/editor';
import { HomeScreen } from '../screens/home';
import type { IntroStrings } from '../strings';

/*
 * The phone world (S4 to S8): one phone, the app on it, and one camera that
 * moves over it from the moment it rises into the frame to the moment it
 * recedes for the mark. Home, where a link is pasted and İndir pressed;
 * İndirmeler, where it downloads and is saved; Düzenle, where the video is
 * picked and cut. The screens draw themselves from the film frame; this owns
 * what they share -- the camera, the phone, the tab bar and the change from
 * one screen to the next -- and the two captions set over it.
 */

const FROM = 260;
const DURATION = 389;

/** A camera position: the screen point in the middle of the frame, in u, and the magnification. */
type Shot = readonly [fx: number, fy: number, m: number];

/*
 * The storyboard's named shots, where the renders moved them:
 *
 * P1 is centred and stops above the tab bar: the frame is narrower than the
 * bar at this magnification, and with the bar in it its sides cut through
 * the outer tabs' labels. It is as low as it can be and still keep the bar
 * out on a 20:9 screen, which also keeps the punch-hole whole at its top
 * edge on a shorter one.
 *
 * P3 is the whole phone rather than a medium shot of the options. The host's
 * Skip button sits in the frame's top-right corner, and a closer shot had to
 * leave the field above the frame: the move there carried the orange Analiz
 * et under Skip, and on a 20:9 screen the field's edge stayed under it. The
 * whole phone is framed for two edges. Its top clears Skip's box on every
 * screen -- by 9 px on a 360x740 one, where the film is scaled down most and
 * Skip is not -- and there is more ground under it than at its sides, so
 * through the film's longest hold on Home it sits in the frame rather than
 * on its edge. At 0.88 it sank against the bottom edge: the camera sets every
 * shot 24 d low to leave the caption band free, and no caption is set over
 * this one. At 0.87 the short screen has room for one of the two, not both.
 * The whole page is in the shot, so İndirmeler arrives in it too, and the
 * camera holds still through İndir and the change of screen.
 *
 * P5 is 6 u to the right, so the row's X is whole at every size: at 214 the
 * frame's edge left a sliver of it that read as a chevron.
 */
const W: Shot = [195, 422, 0.64];
const P1: Shot = [195, 400, 1.15];
const P2: Shot = [195, 300, 1.05];
const P3: Shot = [195, 430, 0.86];
const P5: Shot = [220, 156, 1.45];
const W3: Shot = [195, 410, 0.7];
const P6: Shot = [195, 300, 1];

/**
 * Where the camera is, frame by frame: every move is CAM (EASE.inOut) from
 * one key to the next, and a hold is the same shot twice.
 */
const PATH: readonly (readonly [number, Shot])[] = [
  [260, W],
  [280, W], // the headline is read on the whole phone
  [300, P1], // onto the field, Yapıştır and the tiles
  [316, P1],
  // Onto what was found, over the same frames and on the same curve as the
  // page's own rise (HomeScreen), so the field travels with the camera.
  [330, P2],
  [352, P2],
  [370, P3], // back to the whole result, for the choice
  [430, P3], // held through İndir's press and İndirmeler's arrival
  [446, P5], // onto the download's row
  [488, P5],
  [504, W3], // back, clear of the caption
  [561, W3], // the pick is a quick step on the whole phone, not a shot of its own
  [576, P6], // onto the editor
  [636, P6],
];

const CAMERA = {
  fx: PATH.map(([at, shot]) => [at, shot[0]] as const),
  fy: PATH.map(([at, shot]) => [at, shot[1]] as const),
  // The recede at the end is a zoom alone, so it is only on this track.
  m: [...PATH.map(([at, shot]) => [at, shot[2]] as const), [648, 0.82] as const],
};

/** Screen changes and the tab bar, on the film's clock. */
const TO_DOWNLOADS = 414;
const EDITOR_TAB = 536;
/**
 * Düzenle opens on its empty card: there is no clip open yet. The card is
 * at rest for some nine frames before Video seç is pressed (the editor
 * screen's CHOOSE) and stays four after it.
 */
const TO_PICK = 538;
/** The picked clip is open, cutting past Android's own picker. */
const TO_EDITOR = 559;
const BAR_AWAY = 564;

/** The phone turning on its upright axis, and back, after the download is saved. */
const TURN = { from: 488, duration: 28, degrees: 6 } as const;

/** The world's leaving: the camera recedes and it fades, before the mark. */
const RECEDE = 636;

/**
 * 0 to `degrees` and back to 0 over the turn, sine in and out at both ends:
 * one smooth swing with no corner at the top.
 */
function turnAt(f: number): number {
  const t = ramp(f, TURN.from, TURN.duration, (x) => x);
  return (TURN.degrees * (1 - Math.cos(2 * Math.PI * t))) / 2;
}

/**
 * A spring's last thousandth, rounded off: it is under a tenth of a u on the
 * pill, and the tab bar is left alone as soon as its numbers stop changing,
 * not twenty frames later when the spring finally reads exactly 1.
 */
function settled(value: number): number {
  return Math.abs(1 - value) < 1e-3 ? 1 : value;
}

/**
 * A screen's layer in PHONE_SCREEN 'next' (lib/motion.ts): the old screen
 * leaves 28 u to the left over 6 frames and fades over 4; the new one comes
 * in from 28 u to the right over 10 and fades in over 8, 2 frames late.
 */
function leaving(f: number, at: number) {
  return { x: -28 * ramp(f, at, 6, EASE.in), opacity: 1 - ramp(f, at, 4, EASE.in) };
}

function arriving(f: number, at: number) {
  return { x: 28 * (1 - ramp(f, at, 10)), opacity: ramp(f, at + 2, 8) };
}

/** The middle of the editor's own box, which its arrival scales about. */
const EDITOR_ORIGIN = `50% ${(CONTENT.top + GESTURE.top) / 2}px`;

/**
 * One screen of the app. Every screen is mounted with the scene and kept at
 * opacity 0 outside its window, so none of them is built on a frame the
 * camera is moving.
 */
function ScreenLayer({
  x = 0,
  scale = 1,
  opacity,
  children,
}: {
  x?: number;
  scale?: number;
  opacity: number;
  children: ReactNode;
}) {
  const moved = Math.abs(x) > 0.01;
  const scaled = scale < 1;
  return (
    <div
      className="absolute inset-0"
      style={{
        opacity: opacity < 1 ? opacity : undefined,
        transformOrigin: scaled ? EDITOR_ORIGIN : undefined,
        transform:
          moved || scaled
            ? `${moved ? `translateX(${x}px)` : ''}${scaled ? ` scale(${scale})` : ''}`
            : undefined,
      }}
    >
      {children}
    </div>
  );
}

/**
 * The bar under the screens: the pill on Home, gliding to İndirmeler when
 * İndir sends the download there, pressed over to Düzenle, then stepping
 * down out of the way once a clip is open, since the phone's editor then
 * takes the whole screen. At each change the two tabs' colours cross over
 * as the pill leaves one for the other.
 */
function Tabs({ f, strings }: { f: number; strings: IntroStrings }) {
  const away = ramp(f, BAR_AWAY, 6, EASE.in);
  if (away >= 1) return null;

  const pill =
    settled(springFrom(f, TO_DOWNLOADS, SPRING.glide)) + settled(springFrom(f, EDITOR_TAB, SPRING.glide));
  const selected = f < TO_DOWNLOADS ? 0 : f < EDITOR_TAB ? 1 : 2;
  const change = selected === 0 ? 1 : ramp(f, selected === 1 ? TO_DOWNLOADS : EDITOR_TAB, 4);
  // Arriving at a tab, its icon springs up from slightly small (BottomNav).
  // Düzenle is pressed to get there, and its give is that same dip, taken
  // over the press's four frames instead of at once, so the two are one move.
  const arrival = selected === 1 ? mix(0.86, 1, settled(springFrom(f, TO_DOWNLOADS, SPRING.snap))) : 1;
  const pressed = selected === 2 ? settled(press(f, EDITOR_TAB, 0.86)) : 1;

  return (
    <TabBar
      strings={strings}
      pill={pill}
      selected={selected}
      leaving={selected === 0 ? undefined : selected - 1}
      change={change}
      selectedScale={arrival}
      pressedIndex={selected === 2 ? 2 : undefined}
      pressedScale={pressed}
      away={away}
    />
  );
}

function PhoneWorld({ strings }: { strings: IntroStrings }) {
  const f = useFilmFrame(FROM);

  // The phone rises into the frame with some weight, on a wrapper outside the
  // camera so the camera's first key is already the wide shot. Its fade runs
  // two frames behind the rise and on the camera's curve, so the last values
  // word has all but gone before the headline rising through the same place
  // can be read under it.
  const risen = springFrom(f, FROM, SPRING.stage);
  const y = mix(90, 0, risen);
  const scale = mix(0.96, 1, risen);
  const opacity = ramp(f, FROM + 2, 10, EASE.inOut) * (1 - ramp(f, RECEDE, 12, EASE.in));

  const home = leaving(f, TO_DOWNLOADS);
  const downloadsIn = arriving(f, TO_DOWNLOADS);
  const downloadsOut = leaving(f, TO_PICK);
  const pickIn = arriving(f, TO_PICK);
  // The empty card goes as FADE takes it and the editor comes as EDITOR
  // brings it (lib/motion.ts): in place, the one fading out over the other
  // fading and growing in, with no travel.
  const pickOut = 1 - ramp(f, TO_EDITOR, 5, EASE.in);
  const editor = ramp(f, TO_EDITOR, 10);

  return (
    <Stage>
      <div
        style={{
          position: 'absolute',
          left: 0,
          top: 0,
          transformOrigin: '0 0',
          opacity: opacity < 1 ? opacity : undefined,
          transform: Math.abs(y) > 0.05 ? `translateY(${y}px) scale(${scale})` : undefined,
        }}
      >
        <PhoneCamera
          fx={track(f, CAMERA.fx)}
          fy={track(f, CAMERA.fy)}
          m={trackZoom(f, CAMERA.m)}
          turnY={turnAt(f)}
        >
          <PhoneFrame>
            <AppChrome tabs={<Tabs f={f} strings={strings} />}>
              <ScreenLayer {...home}>
                <HomeScreen f={f} strings={strings} />
              </ScreenLayer>
              <ScreenLayer
                x={downloadsIn.x + downloadsOut.x}
                opacity={downloadsIn.opacity * downloadsOut.opacity}
              >
                <DownloadsScreen f={f} strings={strings} />
              </ScreenLayer>
              <ScreenLayer x={pickIn.x} opacity={pickIn.opacity * pickOut}>
                <EditorPickScreen f={f} strings={strings} />
              </ScreenLayer>
              <ScreenLayer scale={mix(0.995, 1, editor)} opacity={editor}>
                <EditorScreen f={f} strings={strings} />
              </ScreenLayer>
            </AppChrome>
          </PhoneFrame>
        </PhoneCamera>
      </div>

      <Caption text={strings.saved} frame={f} start={494} exit={EDITOR_TAB} />
      <Caption text={strings.trim} frame={f} start={566} exit={RECEDE} />
    </Stage>
  );
}

/*
 * Mounted eleven frames early, at f249 (the values' lf 101): the last word's
 * outline is gone on f248 and nothing changes until its exit on f259, so
 * building the four screens falls on identical frames. Any earlier and it
 * lands on the outline's last steps.
 */
export const scene: IntroScene = { id: 'phone', from: FROM, duration: DURATION, premountFor: 11, Component: PhoneWorld };

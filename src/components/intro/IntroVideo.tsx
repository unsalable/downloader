import { AbsoluteFill, Sequence } from 'remotion';

import type { IntroScene } from './scene';
import { scene as end } from './scenes/end';
import { scene as line } from './scenes/line';
import { scene as phone } from './scenes/phone';
import { scene as share } from './scenes/share';
import { scene as values } from './scenes/values';
import type { IntroStrings } from './strings';

export { INTRO_FPS } from './scene';

/*
 * The film, 23 seconds at 30 fps, after the film the user chose as its model
 * (a remake of a product launch film: one typed line, words in dashed rings,
 * a phone and its close-ups, a mark):
 *
 *   line    0-61    the Home headline, typed           what the app is
 *   share   56-147  the share sheet, our icon, a tap    one way in: share
 *   values  148-267 fast, private, ad-free              what it promises
 *   phone   260-647 paste, choose, download, cut        how it is used
 *   end     636-691 the mark and the name               what to look for
 *
 * The windows overlap: a scene fades in under the one leaving, so each is
 * mounted for its own span of the film's clock rather than cut end to end.
 * Later scenes are drawn over earlier ones.
 */
export const INTRO_SCENES: readonly IntroScene[] = [line, share, values, phone, end];

export const INTRO_FRAMES = 692;

/**
 * When the host's Start button rises in: once the mark has opened, while the
 * name is still settling, so the end reads as an invitation and not a halt.
 */
export const INTRO_CTA_FROM = 668;

/**
 * Frames a scene is mounted, unseen, before its window, unless it says
 * otherwise, so its first frame does not stall on building the scene.
 */
const PREMOUNT = 15;

export function IntroVideo({ strings }: { strings: IntroStrings }) {
  return (
    <AbsoluteFill className="bg-bg">
      {INTRO_SCENES.map(({ id, from, duration, premountFor = PREMOUNT, Component }) => (
        <Sequence key={id} name={id} from={from} durationInFrames={duration} premountFor={premountFor}>
          <Component strings={strings} />
        </Sequence>
      ))}
    </AbsoluteFill>
  );
}

import type { ComponentType } from 'react';

import type { IntroStrings } from './strings';

export const INTRO_FPS = 30;

/**
 * One layer of the film: where it starts on the film's clock, how long it is
 * mounted, and what draws it.
 *
 * Scenes overlap -- one fades in under the one leaving -- so each is mounted
 * for its own window rather than cut end to end. Inside, a scene reads the
 * film's frame with `useFilmFrame(from)` and is written in the storyboard's
 * own frame numbers; its stage is `useStage()`.
 */
export interface IntroScene {
  id: string;
  /** The film frame it is mounted at. */
  from: number;
  /** Frames it stays mounted. */
  duration: number;
  /**
   * Frames it is mounted, unseen, before `from`, so its first frame does not
   * stall. Chosen per scene to fall on a still moment of the one before.
   */
  premountFor?: number;
  Component: ComponentType<{ strings: IntroStrings }>;
}

import type { CSSProperties } from 'react';

import { At, EASE, ramp, Stage, TypeIn, useFilmFrame } from '../kit';
import type { IntroScene } from '../scene';
import type { IntroStrings } from '../strings';

/*
 * The opening line: the Home screen's own headline, typed onto an empty page
 * a character at a time, held, and let go. The same words are on the phone
 * when it arrives, so the first thing the film says is what the app says.
 *
 * Nothing else is on screen and nothing moves but the letters, so every move
 * after it reads as the camera's.
 */

const FROM = 0;
const DURATION = 62;

/** Six empty frames first, a breath before anything is said. */
const TYPE = { start: 6, stagger: 1.2, settle: 10 } as const;

/**
 * The line leaves over f54-61 and is gone on f61, the last frame it is
 * mounted, so unmounting it cannot be seen.
 */
const EXIT = { at: 54, frames: 7 } as const;

const LINE: CSSProperties = {
  fontSize: 21,
  fontWeight: 500,
  letterSpacing: '-0.011em',
  lineHeight: 1.2,
  color: 'var(--text-primary)',
};

function Line({ strings }: { strings: IntroStrings }) {
  const frame = useFilmFrame(FROM);
  const text = strings.heroTitle;
  const sharp = TYPE.start + (text.length - 1) * TYPE.stagger + TYPE.settle;
  const gone = ramp(frame, EXIT.at, EXIT.frames, EASE.in);
  if (gone >= 1) return null;

  return (
    <Stage>
      <At
        style={{
          ...LINE,
          opacity: 1 - gone,
          filter: gone > 0 ? `blur(${gone * 8}px)` : undefined,
          transform: `translate(-50%, -50%)${gone > 0 ? ` scale(${1 + gone * 0.04})` : ''}`,
        }}
      >
        {/* Handed a frame that stops once every character is sharp, so through
            the hold and the exit only the wrapper's style changes. */}
        <TypeIn text={text} frame={Math.min(frame, sharp)} {...TYPE} />
      </At>
    </Stage>
  );
}

export const scene: IntroScene = { id: 'line', from: FROM, duration: DURATION, Component: Line };

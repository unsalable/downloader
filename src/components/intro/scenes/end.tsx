import type { CSSProperties } from 'react';

import { At, MarkIris, mix, ramp, SPRING, springFrom, Stage, TypeIn, useFilmFrame } from '../kit';
import type { IntroScene } from '../scene';
import type { IntroStrings } from '../strings';

/*
 * The end: the mark opens like a lens -- the same aperture the viewer saw on
 * the share target -- and the name types in under it, echoing the opening
 * line. The last frame is the one the player holds while the real button
 * rises over it, so the two sit as one group a little above the middle: the
 * top stays clear of Skip and everything below the name is left to the button.
 *
 * Mounted from f636, eight frames before anything shows, so the mark and the
 * name are already mounted and laid out when they first appear. The phone
 * leaving underneath belongs to the phone's scene.
 */

const FROM = 636;
const DURATION = 56;

/**
 * The disc's spring and its iris start with the phone's recede; the disc
 * itself only shows from f647, once the phone is down to a tenth, so the
 * mark is not seen stamped on the editor's picture -- a second sun beside
 * the one in the landscape.
 */
const MARK = { size: 72, y: -50, from: 644, open: 646, shown: 647 } as const;
const NAME = { y: 22, start: 654, stagger: 1, settle: 10 } as const;

const NAME_STYLE: CSSProperties = {
  fontSize: 21,
  fontWeight: 600,
  letterSpacing: '-0.015em',
  lineHeight: 1.2,
  color: 'var(--text-primary)',
};

function End({ strings }: { strings: IntroStrings }) {
  const frame = useFilmFrame(FROM);

  const shown = ramp(frame, MARK.shown, 6);
  const scale = mix(0.9, 1, springFrom(frame, MARK.from, SPRING.settle));
  const open = ramp(frame, MARK.open, 24);

  const text = strings.appName;
  const sharp = NAME.start + (text.length - 1) * NAME.stagger + NAME.settle;

  return (
    <Stage>
      <div
        style={{
          position: 'absolute',
          left: -MARK.size / 2,
          top: MARK.y - MARK.size / 2,
          width: MARK.size,
          height: MARK.size,
          // A flex box, so the svg is not set on a text baseline and sits exactly in it.
          display: 'flex',
          opacity: shown,
          transform: Math.abs(1 - scale) > 1e-3 ? `scale(${scale})` : undefined,
        }}
      >
        <MarkIris size={MARK.size} open={open} />
      </div>
      <At y={NAME.y} style={NAME_STYLE}>
        {/* The name is the same in every language, and is English. */}
        <span lang="en">
          {/* Handed a frame that stops once every character is sharp, so
              through the hold the glyphs' styles stop changing. */}
          <TypeIn
            text={text}
            frame={Math.min(frame, sharp)}
            start={NAME.start}
            stagger={NAME.stagger}
            settle={NAME.settle}
          />
        </span>
      </At>
    </Stage>
  );
}

export const scene: IntroScene = { id: 'end', from: FROM, duration: DURATION, Component: End };

import { memo, type CSSProperties } from 'react';

import { At, DashedCircle, DashedLine, EASE, mix, ramp, Stage, track, trackZoom, useFilmFrame, useStage } from '../kit';
import type { IntroScene } from '../scene';
import type { IntroStrings } from '../strings';

/*
 * The three promises, one word at a time along a line the camera travels:
 * fast, in a ring whose dashes drift on their own; private, in a ring that
 * closes; ad-free, on its own with nothing round it. The words are the ones
 * the app already welcomes people with.
 *
 * The camera does the telling. It flies in rolled and too close, lands on the
 * first ring, and pans to each next one as it slides in from the right; each
 * word comes out of focus, hollow, and fills as the camera settles on it.
 *
 * The storyboard writes this chapter on a clock of its own, from 0 at film
 * frame 148, so every number below is on that clock.
 */

const FROM = 148;
const DURATION = 120;

/** Where the words sit on the line, in d; the stage starts centred on the first. */
const NODES = [0, 520, 1040] as const;
const RING = 236;

/**
 * The dashed line between the words: out of frame to the left of the first,
 * ring to ring, and from the second toward the third, stopping short of it.
 *
 * Each one stands a gap clear of the rings and is a whole number of dashes
 * long, so it neither touches a ring nor ends on a stub of a dash: a line
 * that ran right to the stroke drew a small cross where the two met.
 */
const DASH = 7;
const GAP = 8;
const CLEAR = RING / 2 + 11;
/** The length of `count` dashes and the gaps between them. */
const dashes = (count: number) => count * (DASH + GAP) - GAP;
const CONNECTORS = [
  [-CLEAR - dashes(38), -CLEAR],
  [CLEAR, CLEAR + dashes(18)],
  [NODES[1] + CLEAR, NODES[1] + CLEAR + dashes(17)],
] as const;

/** Each word starts to resolve here: the first while the camera flies in, the others while it pans onto them. */
const WORD_AT = [4, 40, 78] as const;

const CAMERA = {
  zoom: [[0, 1.9], [22, 1]],
  roll: [[0, -14], [22, 0]],
  pan: [[0, 0], [32, 0], [50, NODES[1]], [72, NODES[1]], [90, NODES[2]]],
} as const;

/** The second ring closing: dash and gap trade so the period stays 15 and the pattern never jumps. */
const SEAL = { at: 56, frames: 14 } as const;
/** The line goes while the camera leaves the second ring, so the last word ends alone. */
const LINES_OUT = { at: 78, frames: 12 } as const;
/** The last word leaves over 110-118 and is gone on 119, the last frame this is mounted. */
const EXIT = { at: 110, frames: 9 } as const;

/** Degrees a frame the first ring's dashes travel: the one thing here that moves by itself. */
const DRIFT = 0.35;

const HAIRLINE = { stroke: 'var(--text-tertiary)', opacity: 0.5 } as const;

const WORD: CSSProperties = {
  display: 'grid',
  fontSize: 46,
  fontWeight: 500,
  letterSpacing: '-0.02em',
  lineHeight: 1.2,
};

/*
 * A dash pattern whose period does not divide the ring leaves one short dash
 * where it wraps, and on the drifting ring that stub goes round with it. The
 * period is stretched by a fraction of a pixel so a whole number of dashes
 * fits.
 */
function fitted(dash: number, gap: number, strokeWidth: number) {
  const around = Math.PI * (RING - strokeWidth);
  const period = dash + gap;
  const stretch = around / (period * Math.round(around / period));
  return { dash: dash * stretch, gap: gap * stretch };
}

const Ring = memo(function Ring({
  x,
  turn,
  dash,
  gap,
  strokeWidth,
  opacity,
}: {
  x: number;
  turn: number;
  dash: number;
  gap: number;
  strokeWidth: number;
  opacity: number;
}) {
  return (
    <At x={x}>
      <DashedCircle
        size={RING}
        turn={turn}
        stroke={HAIRLINE.stroke}
        strokeWidth={strokeWidth}
        opacity={opacity}
        {...fitted(dash, gap, strokeWidth)}
        style={{ display: 'block' }}
      />
    </At>
  );
});

const Connectors = memo(function Connectors({ opacity }: { opacity: number }) {
  if (opacity <= 0) return null;
  return (
    <>
      {CONNECTORS.map(([start, end]) => (
        <DashedLine
          key={start}
          width={end - start}
          stroke={HAIRLINE.stroke}
          opacity={HAIRLINE.opacity * opacity}
          style={{ position: 'absolute', left: start, top: -1, display: 'block' }}
        />
      ))}
    </>
  );
});

interface WordLook {
  /** The wrapper's blur, in px; 0 once it is sharp. */
  blur: number;
  /** The outline and its knockout. */
  line: number;
  fill: number;
  opacity: number;
  scale: number;
}

/**
 * A word that resolves from hollow to solid (the storyboard's OutlineWord):
 * a stroked outline, the same word over it in the page's colour, and the
 * filled word on top, all under one blur. The stroke sits on the glyphs'
 * edges, half in and half out; the knockout covers the inner half and the
 * seams where a variable face's contours overlap, so what shows is a clean
 * outline one pixel wide.
 *
 * Its props stop changing once the word has settled, so from then on it is
 * not drawn again while the camera moves round it.
 */
const OutlineWord = memo(function OutlineWord({ x, text, blur, line, fill, opacity, scale }: WordLook & { x: number; text: string }) {
  // Positioned, so the layers paint in the order written. A layer with an
  // opacity below 1 is otherwise painted after its unpositioned siblings,
  // which put the half-strength outline on top of the knockout and the fill.
  const layer: CSSProperties = { gridArea: '1 / 1', position: 'relative' };
  return (
    <At
      x={x}
      style={{
        ...WORD,
        opacity,
        filter: blur > 0 ? `blur(${blur}px)` : undefined,
        transform: `translate(-50%, -50%)${scale !== 1 ? ` scale(${scale})` : ''}`,
      }}
    >
      {line > 0 && (
        <>
          <span
            style={{
              ...layer,
              color: 'transparent',
              WebkitTextStroke: '2px var(--text-primary)',
              opacity: 0.5 * line,
            }}
          >
            {text}
          </span>
          <span style={{ ...layer, color: 'var(--bg)', opacity: line }}>{text}</span>
        </>
      )}
      <span style={{ ...layer, color: 'var(--text-primary)', opacity: fill }}>{text}</span>
    </At>
  );
});

/**
 * How a word looks `lf` frames into the chapter, having started to resolve
 * at `at`: out of a 10px blur over 16 frames, the outline there in 8 and
 * gone again by 22, the fill in from 10 to 20. Null before it starts.
 */
function resolve(lf: number, at: number): WordLook | null {
  if (lf < at) return null;
  const sharp = ramp(lf, at, 16);
  return {
    blur: sharp < 1 ? 10 * (1 - sharp) : 0,
    line: ramp(lf, at, 8) * (1 - ramp(lf, at + 14, 8, EASE.in)),
    fill: ramp(lf, at + 10, 10),
    opacity: 1,
    scale: 1,
  };
}

function Values({ strings }: { strings: IntroStrings }) {
  const lf = useFilmFrame(FROM) - FROM;
  const { width } = useStage();

  const z = trackZoom(lf, CAMERA.zoom, EASE.out);
  const roll = track(lf, CAMERA.roll, EASE.out);
  const cx = track(lf, CAMERA.pan);

  // Only what is near the frame is drawn. Roll turns the line, so its reach
  // across the stage is judged along the line; a ring's radius and a little
  // more is allowed for what sticks out from a node.
  const across = Math.abs(Math.cos((roll * Math.PI) / 180));
  const near = (x: number) => (Math.abs(x - cx) * across - RING / 2 - 12) * z < width / 2;

  const seal = ramp(lf, SEAL.at, SEAL.frames);
  const gone = ramp(lf, EXIT.at, EXIT.frames, EASE.in);
  const words = [strings.fast, strings.private, strings.adFree];

  return (
    <Stage>
      <div
        style={{
          position: 'absolute',
          left: 0,
          top: 0,
          transformOrigin: '0 0',
          transform: `rotate(${roll}deg) scale(${z}) translate(${-cx}px, 0px)`,
          opacity: ramp(lf, 0, 6),
        }}
      >
        <Connectors opacity={1 - ramp(lf, LINES_OUT.at, LINES_OUT.frames, EASE.in)} />
        {near(NODES[0]) && (
          <Ring x={NODES[0]} turn={DRIFT * lf} dash={DASH} gap={GAP} strokeWidth={1.25} opacity={HAIRLINE.opacity} />
        )}
        {near(NODES[1]) && (
          <Ring
            x={NODES[1]}
            turn={0}
            dash={mix(DASH, DASH + GAP, seal)}
            gap={mix(GAP, 0, seal)}
            strokeWidth={mix(1.25, 1.5, seal)}
            opacity={mix(HAIRLINE.opacity, 0.85, seal)}
          />
        )}
        {NODES.map((x, index) => {
          let look = resolve(lf, WORD_AT[index]!);
          if (!look || !near(x)) return null;
          // Long sharp by the time it leaves, so the exit's blur is the only one.
          if (index === NODES.length - 1 && gone > 0) {
            look = { ...look, blur: gone * 8, opacity: 1 - gone, scale: 1 + gone * 0.04 };
          }
          return <OutlineWord key={x} x={x} text={words[index]!} {...look} />;
        })}
      </div>
    </Stage>
  );
}

export const scene: IntroScene = { id: 'values', from: FROM, duration: DURATION, Component: Values };

/**
 * The ruler's scale, chosen for the zoom rather than fixed.
 *
 * A ruler that always counts seconds is unreadable at both ends of the range
 * this timeline covers: fully out, a four-hour file would carry fourteen
 * thousand marks; fully in, a second is four hundred pixels wide and the ruler
 * says almost nothing. So the step is picked from a ladder, by asking which
 * rung is the first that is far enough apart to read -- the same way a map
 * changes what it names as it is zoomed.
 *
 * The bottom of the ladder is measured in frames rather than in seconds,
 * because at that end the frame is the unit the user is working in.
 */

/** A mark has to be this far from its neighbour to be worth drawing at all. */
const MIN_TICK_PX = 8;

/**
 * And a label needs this much room. An mm:ss label at 11.5px is about 42px
 * wide; the rest is the gap that keeps two of them from reading as one.
 */
const MIN_LABEL_PX = 72;

const SECOND_STEPS = [0.5, 1, 2, 5, 10, 15, 30, 60, 120, 300, 600, 1800, 3600];

/** The ladder, in seconds, for a file at `fps`. */
export function ladderFor(fps: number | null): number[] {
  const frame = fps && fps > 0 ? 1 / fps : null;
  if (!frame) return SECOND_STEPS;
  // Frame-sized rungs only survive while they are still smaller than the first
  // rung above them; at 60 fps ten frames is a sixth of a second, at 12 fps it
  // is most of one, and a ladder that goes backwards is not a ladder.
  const frames = [frame, frame * 2, frame * 5, frame * 10].filter((step) => step < 0.5);
  return [...frames, ...SECOND_STEPS];
}

export interface Ruler {
  /** Distance between the small marks, in seconds. */
  minorStep: number;
  /** Distance between the numbered marks, always a multiple of the minor. */
  labelStep: number;
}

export function rulerFor(pps: number, fps: number | null): Ruler {
  const ladder = ladderFor(fps);
  const first = (minimum: number) =>
    ladder.find((step) => step * pps >= minimum) ?? ladder[ladder.length - 1]!;

  const minorStep = first(MIN_TICK_PX);
  let labelStep = first(MIN_LABEL_PX);
  // A label that does not land on a mark reads as a third scale. Nudge it up
  // the ladder until it is a whole multiple of the one underneath it.
  if (labelStep < minorStep) labelStep = minorStep;
  const factor = Math.round(labelStep / minorStep);
  labelStep = minorStep * Math.max(1, factor);
  return { minorStep, labelStep };
}

/**
 * Every mark in the visible window, and no more: a ruler that laid out a whole
 * four-hour file at forty pixels to the second would be half a million
 * elements, all but a hundred of them off screen.
 */
export function ticksIn(
  startSec: number,
  endSec: number,
  step: number,
): number[] {
  if (step <= 0 || endSec <= startSec) return [];
  const first = Math.ceil(startSec / step) * step;
  const out: number[] = [];
  // A hard ceiling as well as the window, because a step that has somehow come
  // out tiny must not be able to lock the interface up while it counts.
  for (let t = first; t <= endSec && out.length < 4096; t += step) {
    // Re-derived from the index rather than accumulated, so a step of 1/30
    // does not drift a frame out over ten thousand marks.
    out.push(Math.round((t / step)) * step);
  }
  return out;
}

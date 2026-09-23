/**
 * The timeline's coordinate system: seconds to pixels, and back.
 *
 * One number holds the whole of it -- pixels per second -- and every position
 * on the dock is derived from it rather than stored. That is what keeps the
 * ruler, the filmstrip, the waveform and the handles from ever disagreeing
 * about where a second is: there is only one answer, and they all ask it.
 */

/**
 * How far in the timeline will go: four hundred pixels to the second, which at
 * 30 fps is thirteen pixels a frame -- enough to put a cut between two frames
 * with a pointer, and past the point where more would only be emptier.
 */
export const MAX_PPS = 400;

/**
 * And how far out. Not zero: a file long enough to make the fit scale smaller
 * than this is one where a single pixel already covers a minute, and below that
 * the track stops being a picture of anything.
 */
export const MIN_PPS = 0.05;

/** One notch of the wheel, or of the + and - keys. */
export const ZOOM_STEP = 1.15;

export function clampPps(pps: number): number {
  if (!Number.isFinite(pps)) return MIN_PPS;
  return Math.min(MAX_PPS, Math.max(MIN_PPS, pps));
}

/** The scale at which the whole file is exactly as wide as the track. */
export function fitPps(durationSec: number, viewportWidth: number): number {
  if (durationSec <= 0 || viewportWidth <= 0) return MIN_PPS;
  return clampPps(viewportWidth / durationSec);
}

export interface ZoomResult {
  pps: number;
  scrollLeft: number;
}

/**
 * Zoom about a fixed point.
 *
 * The second under the pointer has to still be under the pointer afterwards,
 * or the gesture feels like the timeline lurching rather than like the user
 * leaning in. `anchorX` is measured from the left edge of the visible track,
 * not of the content, which is why the scroll offset is part of both sides of
 * the sum.
 */
export function zoomAbout(
  pps: number,
  scrollLeft: number,
  anchorX: number,
  factor: number,
  durationSec: number,
  viewportWidth: number,
): ZoomResult {
  const next = clampPps(Math.max(pps * factor, fitPps(durationSec, viewportWidth)));
  const seconds = (scrollLeft + anchorX) / pps;
  const maxScroll = Math.max(0, durationSec * next - viewportWidth);
  return {
    pps: next,
    scrollLeft: Math.min(Math.max(seconds * next - anchorX, 0), maxScroll),
  };
}

/**
 * Zoom about a position in the file rather than a position on screen, for the
 * keyboard, which has no pointer to lean on. The playhead is what the user is
 * looking at, so the playhead is what stays put.
 */
export function zoomAboutTime(
  pps: number,
  seconds: number,
  factor: number,
  durationSec: number,
  viewportWidth: number,
): ZoomResult {
  const next = clampPps(Math.max(pps * factor, fitPps(durationSec, viewportWidth)));
  const maxScroll = Math.max(0, durationSec * next - viewportWidth);
  return {
    pps: next,
    scrollLeft: Math.min(Math.max(seconds * next - viewportWidth / 2, 0), maxScroll),
  };
}

/** Keep a moving playhead on screen without dragging the view about under it. */
export function scrollToShow(
  seconds: number,
  pps: number,
  scrollLeft: number,
  viewportWidth: number,
  durationSec: number,
): number {
  const x = seconds * pps;
  const maxScroll = Math.max(0, durationSec * pps - viewportWidth);
  // A margin, so the playhead is never pinned against the edge it is heading
  // for: it should have somewhere to travel to after the view catches up.
  const margin = Math.min(120, viewportWidth * 0.15);
  if (x < scrollLeft + margin) return Math.min(Math.max(x - margin, 0), maxScroll);
  if (x > scrollLeft + viewportWidth - margin) {
    return Math.min(Math.max(x - viewportWidth + margin, 0), maxScroll);
  }
  return scrollLeft;
}

/**
 * Landing on something rather than near it.
 *
 * The radius is given in pixels and converted with the current scale, so
 * snapping stays as strong as it looks however far the timeline is zoomed: six
 * pixels is six pixels whether that is a tenth of a second or a minute. A
 * radius fixed in seconds would be unusable at one end of the range and
 * inescapable at the other.
 */

/** How close is close enough, on screen. */
export const SNAP_PX = 6;

export interface SnapTarget {
  seconds: number;
  /** What it is, so the interface can say why the mark jumped. */
  kind: 'edge' | 'playhead' | 'bounds' | 'keyframe';
}

export interface SnapResult {
  seconds: number;
  /** The target it landed on, or null when it landed where it was dropped. */
  hit: SnapTarget | null;
}

/**
 * Snap `seconds` to the nearest target within the radius.
 *
 * Ties go to the earlier target, and to the earlier entry in the list, which is
 * why the caller puts the bounds and the playhead before the edges: at the very
 * start of a file the beginning of the file and the beginning of the first
 * piece are the same instant, and "the start" is the more useful thing to have
 * been told.
 */
export function snap(
  seconds: number,
  targets: SnapTarget[],
  pps: number,
  enabled: boolean,
): SnapResult {
  if (!enabled || pps <= 0 || targets.length === 0) return { seconds, hit: null };

  const radius = SNAP_PX / pps;
  let best: SnapTarget | null = null;
  let bestDistance = Infinity;

  for (const target of targets) {
    const distance = Math.abs(target.seconds - seconds);
    if (distance <= radius && distance < bestDistance) {
      best = target;
      bestDistance = distance;
    }
  }

  return best ? { seconds: best.seconds, hit: best } : { seconds, hit: null };
}

/**
 * Round to the nearest frame.
 *
 * Applied after snapping rather than before: a mark that has landed on a piece's
 * edge must stay exactly on it, and an edge is wherever the last drag left it
 * rather than a whole number of frames from the start of the file.
 */
export function toFrame(seconds: number, fps: number | null): number {
  if (!fps || fps <= 0 || !Number.isFinite(seconds)) return seconds;
  return Math.round(seconds * fps) / fps;
}

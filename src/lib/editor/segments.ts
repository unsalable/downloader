/**
 * What survives the cut, as a list rather than as a pair of marks.
 *
 * The obvious model -- an in point, an out point and a set of split positions,
 * with the dropped pieces held as indexes into it -- falls apart the moment a
 * new split is made: every index after it shifts by one, and the pieces the
 * user had already thrown away quietly become different pieces. Keeping the
 * kept ranges themselves, each with an identity of its own, means a split is a
 * replacement and a deletion is a removal, and neither can disturb anything it
 * did not touch. It is also exactly what the export asks for.
 *
 * Everything here is pure and total: a call that cannot be honoured returns the
 * list it was given, so a caller never has to check before asking.
 */

import type { EditSegment } from '@/types';

/** A kept range, with the identity that makes it survive its neighbours. */
export interface Cut extends EditSegment {
  id: string;
}

/**
 * The shortest piece worth keeping. Matches the floor the backend enforces, so
 * a split the interface allows is never one the export refuses -- and below it
 * the two edges are the same edge the user has not finished dragging.
 */
export const MIN_CUT_SEC = 0.05;

let counter = 0;

function nextId(): string {
  counter += 1;
  return `cut-${counter}`;
}

/** A file opens whole: one piece, the length of the file. */
export function wholeFile(durationSec: number): Cut[] {
  return [{ id: nextId(), startSec: 0, endSec: Math.max(durationSec, MIN_CUT_SEC) }];
}

/** Seconds that would be written, which is not the same as the span covered. */
export function keptLength(cuts: Cut[]): number {
  return cuts.reduce((total, cut) => total + Math.max(0, cut.endSec - cut.startSec), 0);
}

/** The piece that holds `seconds`, or null when the playhead is over a gap. */
export function cutAt(cuts: Cut[], seconds: number): Cut | null {
  return cuts.find((cut) => seconds >= cut.startSec && seconds <= cut.endSec) ?? null;
}

/**
 * Split whichever piece holds `seconds` into two.
 *
 * Refused at the very edges of a piece, where the second half would be shorter
 * than the floor: the user pressing the key at a boundary means "here", and the
 * honest answer is that there is nothing to divide rather than a sliver that
 * the export would then reject.
 */
export function splitAt(cuts: Cut[], seconds: number): Cut[] {
  const index = cuts.findIndex(
    (cut) => seconds > cut.startSec + MIN_CUT_SEC && seconds < cut.endSec - MIN_CUT_SEC,
  );
  if (index < 0) return cuts;

  const cut = cuts[index]!;
  const left: Cut = { id: nextId(), startSec: cut.startSec, endSec: seconds };
  const right: Cut = { id: nextId(), startSec: seconds, endSec: cut.endSec };
  return [...cuts.slice(0, index), left, right, ...cuts.slice(index + 1)];
}

/**
 * Drop one piece. The last one cannot go: an export of nothing is not a
 * shorter video, it is a file with no frames in it, and the interface disables
 * the control rather than letting the ask be made and then refused.
 */
export function removeCut(cuts: Cut[], id: string): Cut[] {
  if (cuts.length <= 1) return cuts;
  const next = cuts.filter((cut) => cut.id !== id);
  return next.length === cuts.length ? cuts : next;
}

/**
 * Move one edge, stopped by its neighbours rather than pushing them.
 *
 * A piece may be dragged into the gap its neighbour left behind but never
 * through the neighbour itself, which is what keeps the list sorted and
 * non-overlapping without ever having to sort it.
 */
export function moveEdge(
  cuts: Cut[],
  id: string,
  edge: 'start' | 'end',
  seconds: number,
  durationSec: number,
): Cut[] {
  const index = cuts.findIndex((cut) => cut.id === id);
  if (index < 0) return cuts;
  const cut = cuts[index]!;

  const low = edge === 'start' ? (cuts[index - 1]?.endSec ?? 0) : cut.startSec + MIN_CUT_SEC;
  const high =
    edge === 'start'
      ? cut.endSec - MIN_CUT_SEC
      : (cuts[index + 1]?.startSec ?? Math.max(durationSec, MIN_CUT_SEC));

  const placed = Math.min(Math.max(seconds, low), Math.max(low, high));
  const next = [...cuts];
  next[index] = edge === 'start' ? { ...cut, startSec: placed } : { ...cut, endSec: placed };
  return next;
}

/**
 * The pieces that are being thrown away, for the dimming over the track.
 *
 * Drawn as what is left rather than as what is kept, because a file with
 * nothing cut out of it should look like a plain track and not like a filled
 * one.
 */
export function droppedRanges(cuts: Cut[], durationSec: number): EditSegment[] {
  if (durationSec <= 0) return [];
  const gaps: EditSegment[] = [];
  let cursor = 0;
  for (const cut of cuts) {
    if (cut.startSec > cursor) gaps.push({ startSec: cursor, endSec: cut.startSec });
    cursor = Math.max(cursor, cut.endSec);
  }
  if (cursor < durationSec) gaps.push({ startSec: cursor, endSec: durationSec });
  return gaps;
}

/**
 * Where the playhead should go when it walks off the end of a kept piece.
 *
 * Playback skips the dropped pieces, because what the user is watching is the
 * film they are making rather than the one they started with.
 */
export function nextKeptPosition(cuts: Cut[], seconds: number): number | null {
  const inside = cutAt(cuts, seconds);
  if (inside) return seconds;
  const following = cuts.find((cut) => cut.startSec > seconds);
  return following ? following.startSec : null;
}

/** Every edge the playhead or a handle can land on, sorted, without duplicates. */
export function edgesOf(cuts: Cut[]): number[] {
  const edges = new Set<number>();
  for (const cut of cuts) {
    edges.add(cut.startSec);
    edges.add(cut.endSec);
  }
  return [...edges].sort((a, b) => a - b);
}

/** The export's own shape: identity is ours, not the backend's. */
export function toSegments(cuts: Cut[]): EditSegment[] {
  return cuts.map((cut) => ({ startSec: cut.startSec, endSec: cut.endSec }));
}

import { memo, useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from 'react';
import {
  ChevronFirst,
  ChevronLast,
  Magnet,
  Pause,
  Play,
  Scissors,
  SkipBack,
  SkipForward,
  Trash2,
  ZoomIn,
  ZoomOut,
} from 'lucide-react';

import { Filmstrip } from '@/components/editor/Filmstrip';
import type { Playhead } from '@/components/editor/playhead';
import { Waveform } from '@/components/editor/Waveform';
import { IconButton } from '@/components/ui/IconButton';
import { Tooltip } from '@/components/ui/Tooltip';
import { useTranslation } from '@/i18n';
import { cn } from '@/lib/cn';
import type { Cut } from '@/lib/editor/segments';
import { rulerFor, ticksIn } from '@/lib/editor/ticks';
import { snap, toFrame, type SnapTarget } from '@/lib/editor/snap';
import { MAX_PPS, ZOOM_STEP, clampPps, fitPps, scrollToShow, zoomAbout } from '@/lib/editor/zoom';
import { formatDuration, formatTimecode } from '@/lib/format';
import { IS_MOBILE } from '@/lib/platform';
import type { FilmstripData, WaveformData } from '@/types';

/**
 * The bottom half of the editor: the file laid out in time, and everything the
 * user does to it there.
 *
 * It manages its own viewport rather than scrolling natively. A scroll
 * container would mean keeping four layers -- a ruler, two canvases and a set
 * of absolutely positioned handles -- in step with a scrollLeft that the
 * browser owns and changes without telling us, and the canvases would each have
 * to be as wide as the content, which at four hundred pixels to the second over
 * an hour is a surface no browser will allocate. Holding the left edge and the
 * scale as two numbers instead makes every position on the dock the same small
 * sum, and it is what lets a zoom land exactly on the second under the pointer.
 *
 * The cost is a scrollbar of our own, at the foot. It earns its place: it is
 * also the only view of the whole file once the timeline is zoomed in.
 *
 * On a phone the same dock is worked with a thumb. Its tools move up to the
 * page's transport row, its marks get grab areas a fingertip can find, two
 * fingers zoom it, and the file is laid out with a gutter at either end --
 * so the first and last edges are not pressed against the side of the screen,
 * where Android takes a sideways swipe as its Back gesture.
 */

/** Worked by touch rather than by a pointer and a keyboard. */
const TOUCH = IS_MOBILE;

/** The strip of frames, and the sound under it. */
const VIDEO_TRACK_H = TOUCH ? 56 : 68;
const AUDIO_TRACK_H = TOUCH ? 32 : 40;
const RULER_H = TOUCH ? 20 : 24;
const TOOLBAR_H = 36;
const SCROLLBAR_H = 10;
/** The overview bar draws the same thin line on a phone, but a thumb has to find it. */
const OVERVIEW_TOUCH_H = 22;

/**
 * The grab area of a cut's edge. A 3px target is a 3px target however good it
 * looks, and a fingertip needs the 44 a touch screen asks for.
 */
const HANDLE_W = TOUCH ? 44 : 11;
/**
 * How far a fingertip's grab area reaches outside the piece; the rest of it
 * lies inside. Two pieces that meet share a line, and this way the half of the
 * seam on each side belongs to the piece on that side.
 */
const HANDLE_OUT = 12;

/** The clear space at either end of the file, in pixels. None with a pointer. */
const GUTTER = TOUCH ? 22 : 0;

/** The room an mm:ss label takes on the ruler, its padding included. */
const LABEL_ROOM = 30;

/**
 * The snap radius is set in pixels (see snap.ts), and a fingertip lands less
 * exactly than a pointer: on a phone the scale handed to it is shrunk, which
 * widens the same six pixels to about ten.
 */
const SNAP_REACH = TOUCH ? 0.6 : 1;

type Grip =
  | { kind: 'playhead' }
  | { kind: 'edge'; id: string; edge: 'start' | 'end' }
  | { kind: 'pan'; originX: number; originStart: number }
  | { kind: 'scrollbar'; grabOffset: number };

/**
 * How soon after the first finger a second one still makes the touch a pinch
 * from the start, rather than a scrub that turned into one.
 */
const PINCH_GRACE_MS = 300;

/** Two fingers on the track: how far apart they started, and what was under them. */
interface Pinch {
  distance: number;
  pps: number;
  /** The second that sat between the fingers, which stays between them. */
  anchorSec: number;
}

export interface TimelineDockProps {
  /** The clip on the dock. Another one opens fitted, whatever its length. */
  clipId: string;
  durationSec: number;
  fps: number | null;
  hasAudio: boolean;
  cuts: Cut[];
  selectedId: string | null;
  /** Read, and followed, rather than passed in: it moves on every frame of playback. */
  playhead: Playhead;
  playing: boolean;
  snapping: boolean;
  /** Where a copied stream may begin. Only meaningful in lossless mode. */
  keyframes: number[] | null;
  losslessMode: boolean;
  waveform: WaveformData | null;
  filmstrip: FilmstripData | null;
  waveformWorking: boolean;
  disabled?: boolean;

  onSeek: (seconds: number) => void;
  onSelect: (id: string | null) => void;
  /** `held` is true here, where every move comes from a pointer still down. */
  onDragEdge: (id: string, edge: 'start' | 'end', seconds: number, held?: boolean) => void;
  /** Either side of a pointer drag of an edge, so the whole drag undoes as one step. */
  onDragEdgeStart: () => void;
  onDragEdgeEnd: () => void;
  onSplit: () => void;
  onDelete: () => void;
  onToggleSnapping: () => void;
  onPlayPause: () => void;
  onStep: (frames: number) => void;
  /** The window that is visible, so the page can have it drawn at this scale. */
  onViewChange: (startSec: number, lengthSec: number) => void;
  /** Filled in with the dock's zoom actions, so the page can bind keys to them. */
  zoomRef?: React.MutableRefObject<{ by: (factor: number) => void; fit: () => void } | null>;
}

/**
 * Memoised, and fed stable callbacks by the page: it is the largest thing on
 * the screen, and the page re-renders for reasons that have nothing to do with
 * it -- an export's progress, above all.
 */
export const TimelineDock = memo(function TimelineDock({
  clipId,
  durationSec,
  fps,
  hasAudio,
  cuts,
  selectedId,
  playhead,
  playing,
  snapping,
  keyframes,
  losslessMode,
  waveform,
  filmstrip,
  waveformWorking,
  disabled = false,
  onSeek,
  onSelect,
  onDragEdge,
  onDragEdgeStart,
  onDragEdgeEnd,
  onSplit,
  onDelete,
  onToggleSnapping,
  onPlayPause,
  onStep,
  onViewChange,
  zoomRef,
}: TimelineDockProps) {
  const { t } = useTranslation();

  const tracksRef = useRef<HTMLDivElement>(null);
  const barRef = useRef<HTMLDivElement>(null);
  const lineRef = useRef<HTMLDivElement>(null);
  const grip = useRef<Grip | null>(null);
  /** How far the grabbed edge sat from the pointer when it was pressed. */
  const edgeOffset = useRef(0);
  /** The fingers on the track, by pointer, at their latest x. Touch only. */
  const fingers = useRef(new Map<number, number>());
  const pinch = useRef<Pinch | null>(null);
  /** Where the playhead stood before the finger on the track moved it, and when. Touch only. */
  const scrubbed = useRef<{ from: number; at: number } | null>(null);
  const [width, setWidth] = useState(0);
  const [pps, setPps] = useState(1);
  const [viewStart, setViewStart] = useState(0);
  /** The clip, at the length it was fitted to, whose view is on screen. */
  const [fittedFor, setFittedFor] = useState<string | null>(null);

  const usable = durationSec > 0 ? durationSec : 1;
  // The part of the track the file is laid across: all of it with a pointer,
  // and all but the gutters on a phone. `viewStart` is the second at its left
  // end, which is where the file begins when the view is at the start.
  const span = Math.max(0, width - 2 * GUTTER);
  const viewLength = span > 0 ? span / pps : usable;
  const maxStart = Math.max(0, usable - viewLength);
  // What the two canvases are told to draw, which is everything the track
  // shows, gutters included, rather than the part of it the file reaches. A
  // clip shorter than the track at this scale -- anything under width/MAX_PPS
  // seconds -- has a window longer than itself, and handing the canvases the
  // shortened end would make them spread the file over the whole track while
  // the ruler, the playhead and the handles still sat at seconds * pps.
  const windowStart = viewStart - GUTTER / pps;
  const windowEnd = windowStart + (width > 0 ? width / pps : usable);
  // And the part of the file that is actually on screen.
  const shownStart = Math.max(0, windowStart);
  const shownEnd = Math.min(usable, windowEnd);

  // The track's width is not known until it is laid out, and it changes when
  // the sidebar collapses or the dock is resized -- both of which have to
  // re-fit a timeline the user has never zoomed.
  useEffect(() => {
    const element = tracksRef.current;
    if (!element) return;
    const observer = new ResizeObserver((entries) => {
      const next = entries[0]?.contentRect.width ?? 0;
      setWidth(next);
    });
    observer.observe(element);
    setWidth(element.clientWidth);
    return () => observer.disconnect();
  }, []);

  // A new file opens at the scale that shows all of it. Later width changes
  // leave the user's own zoom alone: re-fitting on every resize would undo the
  // zoom they set every time the window moved.
  //
  // Keyed on the clip and not only on its length. Two clips of one length are
  // still two files, and switching between them kept the last one's zoom and
  // scroll -- a view of seconds the user had never looked at in this one. A
  // layout effect, so the clip switched to is never painted at the old scale.
  //
  // A view still at the scale it was fitted to has not been zoomed, so when
  // the track itself changes width -- the sidebar collapsing, a phone turned
  // on its side -- it is fitted again, or the file would cover half the track.
  // The fitted scale is a pure function of the length and the span, so being
  // exactly on it is the proof that nobody has zoomed since.
  const fitKey = `${clipId}|${durationSec}`;
  const fittedSpan = useRef(0);
  useLayoutEffect(() => {
    if (span <= 0 || durationSec <= 0) return;
    const unzoomed =
      fittedFor === fitKey &&
      span !== fittedSpan.current &&
      pps === fitPps(durationSec, fittedSpan.current);
    if (fittedFor === fitKey && !unzoomed) return;
    setPps(fitPps(durationSec, span));
    setViewStart(0);
    setFittedFor(fitKey);
    fittedSpan.current = span;
  }, [durationSec, fitKey, fittedFor, pps, span]);

  const secondsAt = useCallback(
    (clientX: number, bounded = true): number => {
      const element = tracksRef.current;
      if (!element || pps <= 0) return 0;
      const rect = element.getBoundingClientRect();
      const seconds = viewStart + (clientX - rect.left - GUTTER) / pps;
      return bounded ? Math.min(Math.max(seconds, 0), usable) : seconds;
    },
    [pps, usable, viewStart],
  );

  const xOf = useCallback(
    (seconds: number) => GUTTER + (seconds - viewStart) * pps,
    [pps, viewStart],
  );

  /** A left end the view can have at `scale`: never before the file, nor past its end. */
  const clampView = useCallback(
    (start: number, scale: number = pps) =>
      Math.min(Math.max(start, 0), Math.max(0, usable - span / scale)),
    [pps, usable, span],
  );

  // -- zoom and pan --------------------------------------------------------
  //
  // Bound by hand rather than with onWheel, because a React wheel handler is
  // passive and cannot stop the page from scrolling under a Ctrl+wheel -- which
  // in a webview is the browser's own zoom.
  useEffect(() => {
    const element = tracksRef.current;
    if (!element) return;

    const onWheel = (event: WheelEvent) => {
      if (disabled || span <= 0) return;
      event.preventDefault();

      if (event.ctrlKey || event.metaKey) {
        const rect = element.getBoundingClientRect();
        const anchorX = event.clientX - rect.left - GUTTER;
        const notches = -event.deltaY / 100;
        const next = zoomAbout(
          pps,
          viewStart * pps,
          anchorX,
          Math.pow(ZOOM_STEP, notches),
          usable,
          span,
        );
        setPps(next.pps);
        setViewStart(next.scrollLeft / next.pps);
        return;
      }

      // A trackpad sends horizontal deltas of its own; a wheel only has a
      // vertical one, and on this surface it means the same thing.
      const delta = Math.abs(event.deltaX) > Math.abs(event.deltaY) ? event.deltaX : event.deltaY;
      setViewStart((start) => clampView(start + delta / pps));
    };

    element.addEventListener('wheel', onWheel, { passive: false });
    return () => element.removeEventListener('wheel', onWheel);
  }, [clampView, disabled, pps, span, usable, viewStart]);

  const zoomBy = useCallback(
    (factor: number) => {
      if (span <= 0) return;
      const anchor = Math.min(Math.max(xOf(playhead.get()) - GUTTER, 0), span);
      const next = zoomAbout(pps, viewStart * pps, anchor, factor, usable, span);
      setPps(next.pps);
      setViewStart(next.scrollLeft / next.pps);
    },
    [playhead, pps, span, usable, viewStart, xOf],
  );

  const zoomToFit = useCallback(() => {
    if (span <= 0) return;
    setPps(fitPps(usable, span));
    setViewStart(0);
  }, [usable, span]);

  // The zoom lives here, but the keyboard lives on the page above: the scale is
  // this component's own state and lifting it would put a number on the page
  // that only the dock has any use for.
  useEffect(() => {
    if (!zoomRef) return;
    zoomRef.current = { by: zoomBy, fit: zoomToFit };
    return () => {
      zoomRef.current = null;
    };
  }, [zoomBy, zoomRef, zoomToFit]);

  // -- the playhead ----------------------------------------------------------
  //
  // Placed by hand, as a transform, from the playhead's own subscription: the
  // one part of the dock that moves on every frame of playback moves without
  // the dock being rendered for it. Laid out again whenever the scale or the
  // view changes, and before paint, so it never shows where it was.
  useLayoutEffect(() => {
    const line = lineRef.current;
    if (!line) return;
    const place = () => {
      line.style.transform = `translateX(${xOf(playhead.get())}px)`;
    };
    place();
    return playhead.subscribe(place);
  }, [playhead, xOf]);

  // The view follows the playhead while it moves on its own, and only then:
  // dragging the playhead off the edge is the user saying where to look. It
  // turns a page at a time, so this renders the dock once a page rather than
  // once a frame.
  useEffect(() => {
    if (!playing || span <= 0) return;
    const follow = () => {
      const next = clampView(
        scrollToShow(playhead.get(), pps, viewStart * pps, span, usable) / pps,
      );
      if (next !== viewStart) setViewStart(next);
    };
    follow();
    return playhead.subscribe(follow);
  }, [clampView, playhead, playing, pps, span, usable, viewStart]);

  // What is on screen decides what the backend is asked to draw. Sent after the
  // gesture settles rather than during it: a zoom is thirty frames of changing
  // numbers, and each one would otherwise cancel and restart the work.
  useEffect(() => {
    if (width <= 0 || durationSec <= 0) return;
    const start = Math.max(0, windowStart);
    const timer = window.setTimeout(() => onViewChange(start, windowEnd - start), 180);
    return () => window.clearTimeout(timer);
  }, [durationSec, onViewChange, width, windowEnd, windowStart]);

  // -- snapping ------------------------------------------------------------

  // Everything a mark can land on but the playhead, which moves on its own and
  // is read at the moment a drag asks.
  const edgeTargets = useMemo<SnapTarget[]>(() => {
    const list: SnapTarget[] = [];
    for (const cut of cuts) {
      list.push({ seconds: cut.startSec, kind: 'edge' });
      list.push({ seconds: cut.endSec, kind: 'edge' });
    }
    // A lossless cut can only begin at a keyframe, so in that mode the
    // keyframes are where the marks actually want to land.
    if (losslessMode && keyframes) {
      for (const seconds of keyframes) {
        if (seconds >= shownStart && seconds <= shownEnd) list.push({ seconds, kind: 'keyframe' });
      }
    }
    return list;
  }, [cuts, keyframes, losslessMode, shownEnd, shownStart]);

  /**
   * The targets for one move. The bounds and the playhead come first, so that
   * when a piece's edge sits exactly on the start of the file, "the start" is
   * what the user is told they landed on.
   *
   * A mark cannot snap to where it already is: the playhead is a target for
   * everything else, and for itself it would turn every correction smaller
   * than the snap radius into a press that did nothing. The same goes for the
   * edge being dragged -- left in, it is always the nearest target to itself,
   * and the edge sticks where it is until the pointer has pulled a whole snap
   * radius away, then jumps.
   */
  const targetsFor = useCallback(
    (withPlayhead: boolean, ownEdge: number | null = null): SnapTarget[] => {
      const list: SnapTarget[] = [
        { seconds: 0, kind: 'bounds' },
        { seconds: usable, kind: 'bounds' },
      ];
      if (withPlayhead) list.push({ seconds: playhead.get(), kind: 'playhead' });
      for (const target of edgeTargets) {
        if (ownEdge != null && target.kind === 'edge' && target.seconds === ownEdge) continue;
        list.push(target);
      }
      return list;
    },
    [edgeTargets, playhead, usable],
  );

  const [snapHit, setSnapHit] = useState<number | null>(null);

  const place = useCallback(
    (seconds: number, useTargets: boolean, list: SnapTarget[]): number => {
      const result = useTargets
        ? snap(seconds, list, pps * SNAP_REACH, snapping)
        : { seconds, hit: null };
      setSnapHit(result.hit ? result.seconds : null);
      return toFrame(result.seconds, fps);
    },
    [fps, pps, snapping],
  );

  // -- dragging ------------------------------------------------------------

  const applyGrip = useCallback(
    (current: Grip, event: React.PointerEvent) => {
      if (current.kind === 'playhead') {
        onSeek(place(secondsAt(event.clientX), !event.altKey, targetsFor(false)));
        return;
      }
      if (current.kind === 'edge') {
        const cut = cuts.find((entry) => entry.id === current.id);
        const own = cut ? (current.edge === 'start' ? cut.startSec : cut.endSec) : null;
        // Bounded after the offset rather than before it, or an edge taken a
        // few pixels off its line could never be pulled all the way to the
        // start or the end of the file.
        const seconds = Math.min(
          Math.max(secondsAt(event.clientX, false) + edgeOffset.current, 0),
          usable,
        );
        onDragEdge(
          current.id,
          current.edge,
          place(seconds, !event.altKey, targetsFor(true, own)),
          true,
        );
        return;
      }
      if (current.kind === 'pan') {
        setViewStart(clampView(current.originStart - (event.clientX - current.originX) / pps));
        return;
      }
      // Measured against the bar itself, which is where the thumb's
      // percentages are measured from.
      const bar = barRef.current;
      if (!bar) return;
      const rect = bar.getBoundingClientRect();
      if (rect.width <= 0) return;
      const fraction = (event.clientX - rect.left - current.grabOffset) / rect.width;
      setViewStart(clampView(fraction * usable));
    },
    [clampView, cuts, onDragEdge, onSeek, place, pps, secondsAt, targetsFor, usable],
  );

  /**
   * Let go of whatever is held. An edge drag is one step on the way back, and
   * this is where it is written -- so it has to run however the drag ends,
   * including the ways that are not a pointerup.
   */
  const dragEdgeEnd = useRef(onDragEdgeEnd);
  useEffect(() => {
    dragEdgeEnd.current = onDragEdgeEnd;
  });
  const letGo = useCallback(() => {
    const held = grip.current;
    grip.current = null;
    setSnapHit(null);
    if (held?.kind === 'edge') dragEdgeEnd.current();
  }, []);

  // The dock going away mid-drag, or being disabled under one because an
  // export started, is a release that no pointer event will report.
  useEffect(() => letGo, [letGo]);
  useEffect(() => {
    if (disabled) {
      letGo();
      pinch.current = null;
      fingers.current.clear();
    }
  }, [disabled, letGo]);

  // -- two fingers -----------------------------------------------------------
  //
  // A second finger on the track turns whatever the first one was doing into a
  // pinch: the scale follows the distance between them, and the second that
  // sat between them stays there, so the gesture zooms about what the user is
  // looking at and moving both fingers together pans. Whatever the first
  // finger held is let go first -- an edge it was dragging keeps the step it
  // already made, as one step, and a playhead it had only just placed goes
  // back to where it stood.

  const beginPinch = useCallback(
    (at: number) => {
      const measured = fingerSpan(tracksRef.current, fingers.current);
      if (!measured || pps <= 0) return;
      // The first finger of a pinch lands a moment before the second, and a
      // finger on the track moves the playhead the moment it lands -- so
      // every zoom threw the playhead to wherever that finger happened to
      // touch. It goes back, unless the finger had been scrubbing for a while
      // first: then the position is one the user chose.
      const scrub = grip.current?.kind === 'playhead' ? scrubbed.current : null;
      letGo();
      if (scrub && at - scrub.at < PINCH_GRACE_MS) onSeek(scrub.from);
      pinch.current = {
        distance: Math.max(1, measured.distance),
        pps,
        anchorSec: viewStart + measured.middle / pps,
      };
    },
    [letGo, onSeek, pps, viewStart],
  );

  const applyPinch = useCallback(() => {
    const held = pinch.current;
    const measured = fingerSpan(tracksRef.current, fingers.current);
    if (!held || !measured || span <= 0) return;
    const scale = clampPps(
      Math.max((held.pps * measured.distance) / held.distance, fitPps(usable, span)),
    );
    setPps(scale);
    setViewStart(clampView(held.anchorSec - measured.middle / scale, scale));
  }, [clampView, span, usable]);

  const beginDrag = useCallback(
    (next: Grip) => (event: React.PointerEvent) => {
      if (disabled) return;
      event.preventDefault();
      // A grip sits inside the track, and the track takes a press as "move the
      // playhead". Left to bubble, that press would arrive a moment later and
      // replace the grip -- so an edge could be pressed but never dragged.
      if (next.kind !== 'playhead') event.stopPropagation();
      try {
        event.currentTarget.setPointerCapture(event.pointerId);
      } catch {
        // Not capturable. The drag still works; it simply ends at the edge.
      }
      if (TOUCH && (next.kind === 'playhead' || next.kind === 'edge')) {
        fingers.current.set(event.pointerId, event.clientX);
        if (fingers.current.size > 1) {
          if (!pinch.current) beginPinch(event.timeStamp);
          return;
        }
        scrubbed.current =
          next.kind === 'playhead' ? { from: playhead.get(), at: event.timeStamp } : null;
      }
      // A grip whose release was lost is closed before the next one opens.
      letGo();
      grip.current = next;
      if (next.kind === 'edge') {
        // The edge stays under the point it was taken by rather than jumping
        // to the pointer, so a press that goes nowhere moves nothing -- and
        // leaves nothing on the way back to be undone.
        const cut = cuts.find((entry) => entry.id === next.id);
        const pressed = secondsAt(event.clientX, false);
        edgeOffset.current = cut ? (next.edge === 'start' ? cut.startSec : cut.endSec) - pressed : 0;
        onDragEdgeStart();
        return;
      }
      applyGrip(next, event);
    },
    [applyGrip, beginPinch, cuts, disabled, letGo, onDragEdgeStart, playhead, secondsAt],
  );

  const onPointerMove = useCallback(
    (event: React.PointerEvent) => {
      if (TOUCH && fingers.current.has(event.pointerId)) {
        fingers.current.set(event.pointerId, event.clientX);
        if (pinch.current) {
          applyPinch();
          return;
        }
      }
      if (grip.current) applyGrip(grip.current, event);
    },
    [applyGrip, applyPinch],
  );

  const endDrag = useCallback(
    (event: React.PointerEvent) => {
      try {
        if (event.currentTarget.hasPointerCapture(event.pointerId)) {
          event.currentTarget.releasePointerCapture(event.pointerId);
        }
      } catch {
        // Already released, along with the drag it belonged to.
      }
      if (TOUCH) {
        fingers.current.delete(event.pointerId);
        if (pinch.current) {
          // One finger lifting ends the pinch. The one still down does nothing
          // until it lifts too: taking the scrub back up from wherever it
          // happens to rest would throw the playhead across the file.
          if (fingers.current.size < 2) pinch.current = null;
          return;
        }
      }
      letGo();
    },
    [letGo],
  );

  // -- the ruler -----------------------------------------------------------

  const ruler = useMemo(() => rulerFor(pps, fps), [fps, pps]);
  const minorTicks = useMemo(
    () => ticksIn(shownStart, shownEnd, ruler.minorStep),
    [ruler.minorStep, shownEnd, shownStart],
  );
  const labelTicks = useMemo(() => {
    const ticks = ticksIn(shownStart, shownEnd, ruler.labelStep);
    // A label that would run off the right-hand end is left out rather than
    // cut in half. With a pointer the view scrolls it into reach; a phone's
    // track ends in a gutter too narrow for the last one.
    if (!TOUCH || width <= 0) return ticks;
    return ticks.filter((seconds) => GUTTER + (seconds - viewStart) * pps + LABEL_ROOM <= width);
  }, [pps, ruler.labelStep, shownEnd, shownStart, viewStart, width]);

  const canDelete = cuts.length > 1 && selectedId != null;
  const zoomLabel = pps >= 1 ? `${Math.round(pps)} px/s` : `${(1 / pps).toFixed(0)} s/px`;
  // The file's own extent on the track, which on a phone is what is filled:
  // the gutters either side of it stay bare, and read as the ends of the film.
  const fileLeft = xOf(0);
  const fileRight = xOf(usable);
  const thumbLeft = (viewStart / usable) * 100;
  const thumbWidth = Math.min(100, (viewLength / usable) * 100);

  return (
    <div className="flex min-h-0 flex-1 flex-col">
      {/* -- the tools ------------------------------------------------------ */}
      {/* A phone's are the page's transport row, laid out for a thumb. */}
      {!TOUCH && (
        <div className="flex items-center gap-1 px-3" style={{ height: TOOLBAR_H }}>
          <IconButton
            size="sm"
            icon={<Scissors size={15} />}
            label={`${t('editor.split')} · Ctrl+B`}
            disabled={disabled}
            onClick={onSplit}
          />
          <IconButton
            size="sm"
            icon={<Trash2 size={15} />}
            label={canDelete ? `${t('editor.deleteSegment')} · Delete` : t('editor.lastSegment')}
            disabled={disabled || !canDelete}
            onClick={onDelete}
          />
          <IconButton
            size="sm"
            icon={<Magnet size={15} />}
            label={`${t('editor.snap')} · S`}
            active={snapping}
            disabled={disabled}
            onClick={onToggleSnapping}
          />

          <div className="mx-auto flex items-center gap-1">
            <IconButton
              size="sm"
              icon={<ChevronFirst size={15} />}
              label={`${t('editor.toStart')} · Home`}
              disabled={disabled}
              onClick={() => onSeek(0)}
            />
            <IconButton
              size="sm"
              icon={<SkipBack size={15} />}
              label={`${t('editor.stepBack')} · ←`}
              disabled={disabled}
              onClick={() => onStep(-1)}
            />
            <IconButton
              size="sm"
              icon={playing ? <Pause size={16} /> : <Play size={16} />}
              label={`${playing ? t('editor.pause') : t('editor.play')} · Space`}
              tone="accent"
              disabled={disabled}
              onClick={onPlayPause}
            />
            <IconButton
              size="sm"
              icon={<SkipForward size={15} />}
              label={`${t('editor.stepForward')} · →`}
              disabled={disabled}
              onClick={() => onStep(1)}
            />
            <IconButton
              size="sm"
              icon={<ChevronLast size={15} />}
              label={`${t('editor.toEnd')} · End`}
              disabled={disabled}
              onClick={() => onSeek(usable)}
            />
          </div>

          <IconButton
            size="sm"
            icon={<ZoomOut size={15} />}
            label={`${t('editor.zoomOut')} · −`}
            disabled={disabled || pps <= fitPps(usable, span)}
            onClick={() => zoomBy(1 / ZOOM_STEP)}
          />
          <Tooltip label={`${t('editor.zoomFit')} · Ctrl+0`}>
            <button
              type="button"
              onClick={zoomToFit}
              className={cn(
                'tabular pressable-sm h-7 min-w-[64px] rounded-[7px] px-2 text-[11.5px] text-fg-faint',
                'hover:bg-fill hover:text-fg-muted',
              )}
            >
              {zoomLabel}
            </button>
          </Tooltip>
          <IconButton
            size="sm"
            icon={<ZoomIn size={15} />}
            label={`${t('editor.zoomIn')} · +`}
            disabled={disabled || pps >= MAX_PPS}
            onClick={() => zoomBy(ZOOM_STEP)}
          />
        </div>
      )}

      {/* -- the file, in time ---------------------------------------------- */}
      <div
        ref={tracksRef}
        onPointerDown={beginDrag({ kind: 'playhead' })}
        // An edge holds the pointer once it is grabbed, and what it is sent
        // bubbles up to here; handling it on the edge as well would apply
        // every move twice.
        onPointerMove={onPointerMove}
        onPointerUp={endDrag}
        onPointerCancel={endDrag}
        onLostPointerCapture={endDrag}
        onAuxClick={(event) => event.preventDefault()}
        className={cn(
          'relative min-h-0 flex-1 select-none touch-none overflow-hidden',
          disabled ? 'pointer-events-none opacity-50' : 'cursor-text',
        )}
      >
        {/* The ruler. Two ladders, the minor one only where it can be read. */}
        <div className="relative w-full border-b border-[var(--border)]" style={{ height: RULER_H }}>
          {minorTicks.map((seconds) => (
            <span
              key={`m${seconds}`}
              aria-hidden="true"
              className="absolute bottom-0 w-px bg-fg-faint/25"
              style={{ left: xOf(seconds), height: 5 }}
            />
          ))}
          {labelTicks.map((seconds) => (
            <span
              key={`l${seconds}`}
              aria-hidden="true"
              className="tabular absolute bottom-0 flex flex-col items-start pl-1 text-[11px] leading-[14px] text-fg-faint"
              style={{ left: xOf(seconds) }}
            >
              {formatDuration(seconds)}
              <span className="absolute bottom-0 left-0 h-2 w-px bg-fg-faint/45" />
            </span>
          ))}
        </div>

        <div className="relative px-0" style={{ paddingTop: 6 }}>
          {/* The frames. */}
          <div
            className={cn(
              'relative overflow-hidden',
              GUTTER === 0 && 'rounded-[6px] bg-surface-sunken',
            )}
            style={{ height: VIDEO_TRACK_H }}
          >
            {GUTTER > 0 && <Extent left={fileLeft} right={fileRight} />}
            <Filmstrip
              data={filmstrip}
              windowStartSec={windowStart}
              windowEndSec={windowEnd}
              width={width}
              height={VIDEO_TRACK_H}
            />

            {/* What is being thrown away is dimmed; what is kept is left alone,
                so a file with nothing cut out of it looks like a plain track
                rather than like a selection. */}
            <Dimming cuts={cuts} durationSec={usable} xOf={xOf} />

            {cuts.map((cut) => (
              <CutBlock
                key={cut.id}
                cut={cut}
                left={xOf(cut.startSec)}
                right={xOf(cut.endSec)}
                selected={cut.id === selectedId}
                onSelect={onSelect}
                onGrab={beginDrag}
              />
            ))}

            {/* Where a copied cut would really land. Only drawn when the mode
                makes it true, and only once they are far enough apart to mean
                anything. */}
            {losslessMode &&
              keyframes &&
              pps > 8 &&
              keyframes
                .filter((seconds) => seconds >= shownStart && seconds <= shownEnd)
                .map((seconds) => (
                  <span
                    key={`k${seconds}`}
                    aria-hidden="true"
                    className="absolute top-0 h-1.5 w-px bg-fg/30"
                    style={{ left: xOf(seconds) }}
                  />
                ))}
          </div>

          {/* The sound. */}
          <div
            className={cn(
              'relative mt-1.5 overflow-hidden',
              GUTTER === 0 && 'rounded-[6px] bg-surface-sunken',
            )}
            style={{ height: AUDIO_TRACK_H }}
          >
            {GUTTER > 0 && <Extent left={fileLeft} right={fileRight} />}
            {hasAudio ? (
              <>
                <Waveform
                  data={waveform}
                  windowStartSec={windowStart}
                  windowEndSec={windowEnd}
                  width={width}
                  height={AUDIO_TRACK_H}
                  loading={waveformWorking}
                />
                {waveform == null && waveformWorking && (
                  <span className="absolute inset-0 flex items-center justify-center text-[11.5px] text-fg-faint">
                    {t('editor.readingAudio')}
                  </span>
                )}
                <Dimming cuts={cuts} durationSec={usable} xOf={xOf} />
              </>
            ) : (
              <span className="absolute inset-0 flex items-center justify-center text-[11.5px] text-fg-faint">
                {t('editor.noAudio')}
              </span>
            )}
          </div>
        </div>

        {/* The snap guide, over everything, for as long as the mark is held on
            something. */}
        {snapHit != null && (
          <span
            aria-hidden="true"
            className="pointer-events-none absolute inset-y-0 w-px bg-accent/70"
            style={{ left: xOf(snapHit) }}
          />
        )}

        {/* The playhead. Drawn last so it is visible over the dimming as well
            as over the frames, and moved by its subscription above. */}
        <div
          ref={lineRef}
          aria-hidden="true"
          className="pointer-events-none absolute inset-y-0 left-0 z-10 w-px -translate-x-1/2 bg-fg will-change-transform"
        >
          <span className="absolute -left-[3px] top-0 size-[7px] rounded-[2px] bg-fg" />
        </div>
      </div>

      {/* -- where we are in the whole file --------------------------------- */}
      {TOUCH ? (
        // A thumb's height of track under the same thin line. A press beside
        // the thumb brings the view there and carries on as a drag of it.
        <div
          ref={barRef}
          className={cn('relative shrink-0 touch-none', disabled && 'pointer-events-none')}
          style={{ height: OVERVIEW_TOUCH_H, marginInline: GUTTER }}
          onPointerDown={(event) => {
            const rect = event.currentTarget.getBoundingClientRect();
            const grabOffset = ((thumbWidth / 100) * rect.width) / 2;
            beginDrag({ kind: 'scrollbar', grabOffset })(event);
          }}
          onPointerMove={onPointerMove}
          onPointerUp={endDrag}
          onPointerCancel={endDrag}
        >
          <span
            aria-hidden="true"
            className="pointer-events-none absolute inset-x-0 top-1/2 h-1 -translate-y-1/2 rounded-full bg-fill"
          />
          <div
            role="scrollbar"
            aria-controls="editor-timeline"
            aria-orientation="horizontal"
            aria-valuenow={Math.round((viewStart / Math.max(maxStart, 1e-6)) * 100)}
            tabIndex={-1}
            onPointerDown={(event) => {
              const rect = event.currentTarget.getBoundingClientRect();
              beginDrag({ kind: 'scrollbar', grabOffset: event.clientX - rect.left })(event);
            }}
            className="absolute inset-y-0"
            style={{ left: `${thumbLeft}%`, width: `${thumbWidth}%` }}
          >
            <span
              aria-hidden="true"
              className="pointer-events-none absolute inset-x-0 top-1/2 h-1 -translate-y-1/2 rounded-full bg-fg-faint/45"
            />
          </div>
        </div>
      ) : (
        <div
          ref={barRef}
          className="relative mx-3 my-1.5 shrink-0 rounded-full bg-fill"
          style={{ height: SCROLLBAR_H - 4 }}
          onPointerMove={onPointerMove}
          onPointerUp={endDrag}
          onPointerCancel={endDrag}
        >
          <div
            role="scrollbar"
            aria-controls="editor-timeline"
            aria-orientation="horizontal"
            aria-valuenow={Math.round((viewStart / Math.max(maxStart, 1e-6)) * 100)}
            tabIndex={-1}
            onPointerDown={(event) => {
              const rect = event.currentTarget.getBoundingClientRect();
              beginDrag({ kind: 'scrollbar', grabOffset: event.clientX - rect.left })(event);
            }}
            className="absolute inset-y-0 cursor-grab rounded-full bg-fg-faint/45 active:cursor-grabbing"
            style={{ left: `${thumbLeft}%`, width: `${thumbWidth}%` }}
          />
        </div>
      )}
    </div>
  );
});

/**
 * The first two fingers on the track: how far apart they are, and where the
 * point between them falls on the file's part of it.
 */
function fingerSpan(
  element: HTMLElement | null,
  fingers: Map<number, number>,
): { distance: number; middle: number } | null {
  const [a, b] = [...fingers.values()];
  if (!element || a == null || b == null) return null;
  const rect = element.getBoundingClientRect();
  return { distance: Math.abs(a - b), middle: (a + b) / 2 - rect.left - GUTTER };
}

/** The stretch of a track the file covers, filled; only drawn where there are gutters. */
function Extent({ left, right }: { left: number; right: number }) {
  return (
    <span
      aria-hidden="true"
      className="pointer-events-none absolute inset-y-0 rounded-[6px] bg-surface-sunken"
      style={{ left, width: Math.max(0, right - left) }}
    />
  );
}

/** The parts of the file that will not be in the result. */
const Dimming = memo(function Dimming({
  cuts,
  durationSec,
  xOf,
}: {
  cuts: Cut[];
  durationSec: number;
  xOf: (seconds: number) => number;
}) {
  // Each span is measured between two projected edges rather than from its
  // duration, so the two ends agree with the tracks underneath at every scale
  // and a span that runs off the side is simply clipped by the track.
  const spans: { left: number; width: number }[] = [];
  let cursor = 0;
  for (const cut of cuts) {
    if (cut.startSec > cursor) {
      const left = xOf(cursor);
      spans.push({ left, width: xOf(cut.startSec) - left });
    }
    cursor = Math.max(cursor, cut.endSec);
  }
  if (cursor < durationSec) {
    const left = xOf(cursor);
    spans.push({ left, width: xOf(durationSec) - left });
  }

  return (
    <>
      {spans.map((span, index) => (
        <span
          key={index}
          aria-hidden="true"
          className="pointer-events-none absolute inset-y-0 bg-bg/65"
          style={{ left: span.left, width: Math.max(0, span.width) }}
        />
      ))}
    </>
  );
});

/**
 * One kept piece. Memoised, with the handlers it is given taking the piece's id
 * rather than closing over it: a list of these re-rendering together is what a
 * timeline with a few dozen cuts spends its time on.
 */
const CutBlock = memo(function CutBlock({
  cut,
  left,
  right,
  selected,
  onSelect,
  onGrab,
}: {
  cut: Cut;
  left: number;
  right: number;
  selected: boolean;
  onSelect: (id: string) => void;
  onGrab: (grip: Grip) => (event: React.PointerEvent) => void;
}) {
  // By touch, taking hold of an edge also picks the piece, so the Delete
  // beside the play button means the piece the finger was just on.
  const grab = (edge: 'start' | 'end') => {
    const begin = onGrab({ kind: 'edge', id: cut.id, edge });
    if (!TOUCH) return begin;
    return (event: React.PointerEvent) => {
      onSelect(cut.id);
      begin(event);
    };
  };

  return (
    <div
      role="button"
      tabIndex={0}
      aria-label={`${formatTimecode(cut.startSec)} – ${formatTimecode(cut.endSec)}`}
      aria-pressed={selected}
      onPointerDown={() => {
        // Deliberately allowed to bubble. A kept piece covers its whole stretch
        // of the track, so stopping here would mean a press anywhere on the
        // film selected it and nothing else -- and the playhead could then only
        // be moved in the gaps between pieces. Selecting and seeking are both
        // what the press meant.
        onSelect(cut.id);
      }}
      onKeyDown={(event) => {
        if (event.key === 'Enter' || event.key === ' ') {
          event.preventDefault();
          onSelect(cut.id);
        }
      }}
      className={cn(
        'absolute inset-y-0 rounded-[5px] outline-none',
        'transition-[box-shadow,background-color] duration-150 ease-out-quint',
        selected
          ? 'shadow-[inset_0_0_0_2px_var(--accent)]'
          : 'hover:shadow-[inset_0_0_0_1.5px_var(--border-strong)] focus-visible:shadow-[inset_0_0_0_2px_var(--accent)]',
      )}
      style={{ left, width: Math.max(0, right - left) }}
    >
      <Edge side="start" onPointerDown={grab('start')} />
      <Edge side="end" onPointerDown={grab('end')} />
    </div>
  );
});

/**
 * One edge of one piece. With a pointer the grab area is centred on the line;
 * by touch it is a fingertip wide and lies mostly inside the piece (see
 * HANDLE_OUT), while the handle drawn on the line stays slim.
 */
function Edge({
  side,
  onPointerDown,
}: {
  side: 'start' | 'end';
  onPointerDown: (event: React.PointerEvent) => void;
}) {
  const start = side === 'start';
  return (
    <div
      onPointerDown={onPointerDown}
      className={cn(
        'group absolute inset-y-0 flex cursor-ew-resize touch-none items-center',
        TOUCH
          ? start
            ? 'justify-start'
            : 'justify-end'
          : cn('justify-center', start ? 'left-0 -translate-x-1/2' : 'right-0 translate-x-1/2'),
      )}
      style={
        TOUCH
          ? { width: HANDLE_W, [start ? 'left' : 'right']: -HANDLE_OUT }
          : { width: HANDLE_W }
      }
    >
      <span
        aria-hidden="true"
        className={cn(
          'h-[calc(100%-10px)] w-[5px] shrink-0 rounded-full bg-accent',
          'shadow-[0_0_0_0.5px_rgb(0_0_0/0.12),0_1px_3px_rgb(0_0_0/0.25)]',
          // Brightening on press rather than growing: a handle that changes
          // size moves the mark it stands for.
          'transition-[filter] duration-150 ease-out-quint group-active:brightness-125',
        )}
        // Centred on the line, which sits HANDLE_OUT in from the grab area's
        // outer side.
        style={TOUCH ? { [start ? 'marginLeft' : 'marginRight']: HANDLE_OUT - 2.5 } : undefined}
      />
    </div>
  );
}

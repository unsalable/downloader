import { ChevronDown, FilePlus2, Link2, Play, Redo2, Scissors, Trash2, Undo2, X } from 'lucide-react';
import { memo, type ReactNode } from 'react';
import { random } from 'remotion';

import { formatDuration, formatTimecode } from '@/lib/format';
import { EASE, mix, press, ramp, Ripple, SCREEN, Segmented, Swap, track } from '../kit';
import { CONTENT, GESTURE, Landscape } from '../screens';
import type { IntroStrings } from '../strings';

/*
 * The phone editor (EditorPage's phone branch): first empty, as the Düzenle
 * tab opens it, with the card that asks for a video; then with a clip open,
 * and the one edit the film makes in it: split at two points, delete the
 * piece between them, and watch the length drop.
 *
 * It is the whole screen from the top of the content to the gesture strip,
 * as the real one is once a clip is open and the tab bar has stepped aside.
 * Laid out as the real page is -- the same column, the same heights -- so the
 * numbers below are the page's own: a 56 px bar, the picture at the clip's
 * shape across the width, a 56 px transport, and TimelineDock's phone sizes
 * with the 22 px gutter it keeps at either end of the file.
 */

/** The clip's length in seconds: the 4:32 the downloads scene finished. */
const LENGTH = 272;

// -- the edit, on the film's clock -------------------------------------------

/** Where the two cuts fall, as parts of the clip. */
const LATE = 0.66;
const EARLY = 0.34;
const SPLIT_LATE = 590;
const SPLIT_EARLY = 606;
const REMOVE = 614;
/** What is left once the middle has gone: 272 x 0.68 = 184.96 s, "03:05.0". */
const KEPT = LENGTH * (1 - (LATE - EARLY));

/** The playhead, as a part of the clip: out to the late cut, then back to the early one. */
function playheadAt(f: number): number {
  return track(f, [
    [580, 0],
    [588, LATE],
    [596, LATE],
    [604, EARLY],
  ]);
}

/**
 * The edit's own clock: the frame, held still outside the stretch where
 * anything on this screen moves, so the memoised parts below render once on
 * either side of it instead of on every frame of the film.
 */
function editFrame(f: number): number {
  return Math.min(Math.max(f, 578), REMOVE + 26);
}

// -- the timeline's geometry (TimelineDock, phone) ---------------------------

const TRACK_W = SCREEN.width - 32;
const GUTTER = 22;
const FILE_W = TRACK_W - 2 * GUTTER;
const RULER_H = 20;
const VIDEO_H = 56;
const AUDIO_H = 32;
const TRACK_GAP = 6;
const TRACKS_H = RULER_H + TRACK_GAP + VIDEO_H + TRACK_GAP + AUDIO_H;
const OVERVIEW_H = 22;
/** A filmstrip cell at the frames' own 16:9, as Filmstrip steps them. */
const CELL_W = (VIDEO_H * 16) / 9;
/**
 * The space a cut opens between two pieces. Not in the app, where pieces that
 * meet share a line: here it keeps the first cut in sight once the selection
 * has moved on to the second.
 */
const CUT_GAP = 2;

/** A part of the clip to x on the track, gutters included. */
function xOf(part: number): number {
  return GUTTER + part * FILE_W;
}

// -- the picture ---------------------------------------------------------------

const Still = memo(function Still() {
  return <Landscape style={{ width: '100%', height: '100%' }} />;
});

/**
 * The clip's picture at `at`, a part of its length. The clip is one slow pan
 * across the landscape, so the preview follows the playhead and the filmstrip's
 * cells differ the way a video's frames do. Only a transform changes.
 */
function Picture({ at }: { at: number }) {
  return (
    <div className="absolute inset-0 overflow-hidden">
      <div
        style={{
          position: 'absolute',
          left: 0,
          top: 0,
          width: '120%',
          height: '120%',
          transform: `translate(${-at * (100 / 6)}%, ${-100 / 12}%)`,
        }}
      >
        <Still />
      </div>
    </div>
  );
}

// -- the bar -------------------------------------------------------------------

/**
 * The bar: close, the file's name over its length, and the real bar's
 * trailing controls. Dışa aktar is there although nothing presses it: it is
 * what writes the cut, and without it the delete would seem to change the
 * file.
 */
const Bar = memo(function Bar({ f, undo, strings }: { f: number; undo: number; strings: IntroStrings }) {
  return (
    <div className="flex h-14 shrink-0 items-center gap-1 pl-1.5 pr-3">
      <span className="flex size-11 shrink-0 items-center justify-center rounded-full text-fg">
        <X size={22} />
      </span>
      <div className="min-w-0 flex-1 pl-0.5">
        {/* Named as the download was saved -- the default template,
            '{creator} - {title} [{quality}]' -- and cut short as the real bar
            cuts it. */}
        <p className="truncate text-[15px] font-semibold leading-5 text-fg">
          {strings.sampleChannel} - {strings.sampleTitle} [1080p].mkv
        </p>
        {/* Not truncated like the app's: the value lifts 3 px as it leaves,
            and a clipped line would cut the top off it. It is short enough
            never to need it. */}
        <p className="tabular whitespace-nowrap text-[12.5px] leading-4 text-fg-muted">
          {strings.editorLength}{' '}
          <Swap frame={f} at={REMOVE} from={formatTimecode(LENGTH)} to={formatTimecode(KEPT)} />
        </p>
      </div>
      {/* IconButton at the phone's size: Undo wakes from its disabled 40%
          with the first split, as canUndo would; Redo stays asleep. */}
      <span
        className="inline-flex size-10 shrink-0 items-center justify-center rounded-[8px] text-fg-muted"
        style={{ opacity: mix(0.4, 1, undo) }}
      >
        <Undo2 size={20} />
      </span>
      <BarEnd label={strings.exportVideo} />
    </div>
  );
});

/** Redo, disabled, and Dışa aktar (Button sm primary at the phone's size). */
const BarEnd = memo(function BarEnd({ label }: { label: string }) {
  return (
    <>
      <span className="inline-flex size-10 shrink-0 items-center justify-center rounded-[8px] text-fg-muted opacity-40">
        <Redo2 size={20} />
      </span>
      <span className="ml-1 inline-flex h-10 shrink-0 items-center justify-center rounded-[var(--radius-control)] bg-accent px-3.5 text-[13.5px] font-medium text-accent-fg">
        {label}
      </span>
    </>
  );
});

const Preview = memo(function Preview({ at }: { at: number }) {
  return (
    <div className="flex shrink-0 px-4" style={{ height: (TRACK_W * 9) / 16 }}>
      <div className="relative min-w-0 flex-1 overflow-hidden rounded-[var(--radius-card)]">
        <Picture at={at} />
      </div>
    </div>
  );
});

// -- the transport ---------------------------------------------------------------

/**
 * EditorPage's TouchButton, pressed by the frame: the latest of `presses` that
 * has happened gives the button its pressed fill and scale, and a ripple
 * clipped to its round shape.
 */
function TouchTarget({ f, presses = [], children }: { f: number; presses?: number[]; children: ReactNode }) {
  const at = presses.filter((time) => f >= time).at(-1);
  const scale = at == null ? 1 : press(f, at, 0.94);
  const held = at == null ? 0 : ramp(f, at, 2) * (1 - ramp(f, at + 4, 6, EASE.in));
  return (
    <span
      className="relative flex size-11 shrink-0 items-center justify-center overflow-hidden rounded-full text-fg"
      style={{ transform: scale !== 1 ? `scale(${scale})` : undefined }}
    >
      {held > 0 && <span className="absolute inset-0 rounded-full bg-fill-active" style={{ opacity: held }} />}
      {at != null && <Ripple frame={f} at={at} size={60} />}
      <span className="relative flex">{children}</span>
    </span>
  );
}

const Transport = memo(function Transport({ f }: { f: number }) {
  return (
    <div className="flex h-14 shrink-0 items-center gap-1 px-2">
      <TouchTarget f={f}>
        <Play size={22} fill="currentColor" />
      </TouchTarget>
      <p className="tabular min-w-0 flex-1 truncate pl-1 text-[14px] text-fg">
        {formatTimecode(LENGTH * playheadAt(f))}
        <span className="text-fg-faint"> / {formatTimecode(LENGTH)}</span>
      </p>
      <TouchTarget f={f} presses={[SPLIT_LATE, SPLIT_EARLY]}>
        <Scissors size={20} />
      </TouchTarget>
      <TouchTarget f={f} presses={[REMOVE]}>
        <Trash2 size={20} />
      </TouchTarget>
    </div>
  );
});

// -- the file, in time ------------------------------------------------------------

/** The ruler at the fitted scale: a mark every ten seconds, a number every two minutes. */
const Ruler = memo(function Ruler() {
  const minor = Array.from({ length: Math.floor(LENGTH / 10) + 1 }, (_, index) => index * 10);
  const labels = [0, 120, 240];
  return (
    <div className="relative w-full border-b border-[var(--border)]" style={{ height: RULER_H }}>
      {minor.map((seconds) => (
        <span
          key={`m${seconds}`}
          className="absolute bottom-0 w-px bg-fg-faint/25"
          style={{ left: xOf(seconds / LENGTH), height: 5 }}
        />
      ))}
      {labels.map((seconds) => (
        <span
          key={`l${seconds}`}
          className="tabular absolute bottom-0 flex flex-col items-start pl-1 text-[11px] leading-[14px] text-fg-faint"
          style={{ left: xOf(seconds / LENGTH) }}
        >
          {formatDuration(seconds)}
          <span className="absolute bottom-0 left-0 h-2 w-px bg-fg-faint/45" />
        </span>
      ))}
    </div>
  );
});

/** The frames, laid from the file's start as Filmstrip lays them. */
const VideoStrip = memo(function VideoStrip() {
  const cells = Math.ceil(FILE_W / CELL_W);
  return (
    <div className="relative overflow-hidden" style={{ width: FILE_W, height: VIDEO_H }}>
      {Array.from({ length: cells }, (_, index) => (
        // A pixel wider than its step, under the next cell: cells that only
        // met at a fractional x left a hairline of the track showing between.
        <div key={index} className="absolute top-0" style={{ left: index * CELL_W, width: CELL_W + 1, height: VIDEO_H }}>
          {/* Each cell shows the frame at the middle of its stretch. */}
          <Picture at={Math.min(1, ((index + 0.5) * CELL_W) / FILE_W)} />
        </div>
      ))}
    </div>
  );
});

/**
 * The sound: one path of thin bars, louder and quieter in slow swells as a
 * recording is. Seeded, so every frame and every render draws the same one. In
 * the accent, as Waveform paints it.
 */
const WAVE_PATH = (() => {
  const step = 1.5;
  const bar = 1;
  const centre = AUDIO_H / 2;
  const reach = centre - 1;
  const count = Math.floor(FILE_W / step);
  let path = '';
  for (let index = 0; index < count; index += 1) {
    const at = index / 14;
    const from = Math.floor(at);
    const eased = (1 - Math.cos((at - from) * Math.PI)) / 2;
    const swell = mix(random(`wave-swell-${from}`), random(`wave-swell-${from + 1}`), eased);
    const level = (0.2 + 0.8 * swell) * (0.45 + 0.55 * random(`wave-${index}`));
    const half = Math.max(0.5, reach * level);
    const x = index * step + (step - bar) / 2;
    path += `M${x.toFixed(2)} ${(centre - half).toFixed(2)}h${bar}v${(half * 2).toFixed(2)}h${-bar}z`;
  }
  return path;
})();

const AudioStrip = memo(function AudioStrip() {
  return (
    <svg
      width={FILE_W}
      height={AUDIO_H}
      viewBox={`0 0 ${FILE_W} ${AUDIO_H}`}
      className="block text-accent"
      aria-hidden="true"
    >
      <path d={WAVE_PATH} fill="currentColor" />
    </svg>
  );
});

/** The overview bar under the tracks: the whole file is in view, so its thumb is all of it. */
const Overview = memo(function Overview() {
  return (
    <div className="relative shrink-0" style={{ height: OVERVIEW_H, marginInline: GUTTER }}>
      <span className="absolute inset-x-0 top-1/2 h-1 -translate-y-1/2 rounded-full bg-fill" />
      <span className="absolute inset-x-0 top-1/2 h-1 -translate-y-1/2 rounded-full bg-fg-faint/45" />
    </div>
  );
});

interface Piece {
  from: number;
  to: number;
  left: number;
  right: number;
  radius: string;
}

/**
 * The kept pieces at frame `f`: the whole clip, then two, then three. A cut
 * opens its gap over four frames, and the corners either side of it round as
 * it does, so the split reads as the strip coming apart where the playhead is.
 */
function piecesAt(f: number): Piece[] {
  const cuts = [
    { at: LATE, made: SPLIT_LATE },
    { at: EARLY, made: SPLIT_EARLY },
  ]
    .filter((cut) => f >= cut.made)
    .map((cut) => ({ at: cut.at, open: ramp(f, cut.made, 4) }))
    .sort((a, b) => a.at - b.at);
  const bounds = [{ at: 0, open: 0 }, ...cuts, { at: 1, open: 0 }];
  const pieces: Piece[] = [];
  for (let index = 0; index < bounds.length - 1; index += 1) {
    const start = bounds[index]!;
    const end = bounds[index + 1]!;
    // The file's own ends keep the track's corner; a cut's corner grows with its gap.
    const startRadius = start.at === 0 ? 6 : 5 * start.open;
    const endRadius = end.at === 1 ? 6 : 5 * end.open;
    pieces.push({
      from: start.at,
      to: end.at,
      left: xOf(start.at) + (start.at === 0 ? 0 : (CUT_GAP / 2) * start.open),
      right: xOf(end.at) - (end.at === 1 ? 0 : (CUT_GAP / 2) * end.open),
      radius: `${startRadius}px ${endRadius}px ${endRadius}px ${startRadius}px`,
    });
  }
  return pieces;
}

/** One piece of one track: the strip, cut to the piece, over the track's sunken fill. */
function PieceBox({ piece, dim, children }: { piece: Piece; dim: number; children: ReactNode }) {
  return (
    <div
      className="absolute inset-y-0 overflow-hidden bg-surface-sunken"
      style={{ left: piece.left, width: piece.right - piece.left, borderRadius: piece.radius }}
    >
      <div className="absolute inset-y-0 left-0" style={{ transform: `translateX(${GUTTER - piece.left}px)` }}>
        {children}
      </div>
      {/* What is being thrown away, dimmed as Dimming dims it. */}
      {dim > 0 && <span className="absolute inset-0 bg-bg/65" style={{ opacity: dim }} />}
    </div>
  );
}

/**
 * A selected piece: CutBlock's accent ring and its two handles, centred on the
 * edges. The handles leave out the app's small dark shadow, which is a colour
 * outside the theme; on the frames the accent reads without it.
 */
function Selection({ piece, opacity }: { piece: Piece; opacity: number }) {
  if (opacity <= 0) return null;
  const handle = (x: number) => (
    <span className="absolute w-[5px] rounded-full bg-accent" style={{ left: x - 2.5, top: 5, bottom: 5 }} />
  );
  return (
    <div className="absolute inset-0" style={{ opacity }}>
      <span
        className="absolute inset-y-0 rounded-[5px] shadow-[inset_0_0_0_2px_var(--accent)]"
        style={{ left: piece.left, width: piece.right - piece.left }}
      />
      {handle(xOf(piece.from))}
      {handle(xOf(piece.to))}
    </div>
  );
}

const Timeline = memo(function Timeline({ f }: { f: number }) {
  const pieces = piecesAt(f);
  const middle = pieces.find((piece) => piece.from === EARLY && piece.to === LATE);
  const last = pieces.find((piece) => piece.from === LATE);
  // As splitHere does: each split selects the piece that starts at the cut.
  // Delete takes the selection with it.
  const lastSelected = ramp(f, SPLIT_LATE, 4) * (1 - ramp(f, SPLIT_EARLY, 4));
  const middleSelected = ramp(f, SPLIT_EARLY, 4) * (1 - ramp(f, REMOVE, 5, EASE.in));
  const dropped = ramp(f, REMOVE, 8);
  const dimOf = (piece: Piece) => (piece === middle ? dropped : 0);

  return (
    <div className="flex shrink-0 flex-col px-4 pb-1">
      <div className="relative overflow-hidden" style={{ height: TRACKS_H }}>
        <Ruler />
        <div className="relative" style={{ paddingTop: TRACK_GAP }}>
          <div className="relative overflow-hidden" style={{ height: VIDEO_H }}>
            {/* Keyed by where a piece starts, which a split leaves alone for the
                piece on its left: only the new piece is built on the press. */}
            {pieces.map((piece) => (
              <PieceBox key={piece.from} piece={piece} dim={dimOf(piece)}>
                <VideoStrip />
              </PieceBox>
            ))}
            {last && <Selection piece={last} opacity={lastSelected} />}
            {middle && <Selection piece={middle} opacity={middleSelected} />}
          </div>
          <div className="relative overflow-hidden" style={{ height: AUDIO_H, marginTop: TRACK_GAP }}>
            {pieces.map((piece) => (
              <PieceBox key={piece.from} piece={piece} dim={dimOf(piece)}>
                <AudioStrip />
              </PieceBox>
            ))}
          </div>
        </div>
        {/* The playhead, drawn as TimelineDock draws it, over everything. */}
        <div
          className="absolute inset-y-0 left-0 z-10 w-px -translate-x-1/2 bg-fg"
          style={{ transform: `translateX(${xOf(playheadAt(f))}px)` }}
        >
          <span className="absolute -left-[3px] top-0 size-[7px] rounded-[2px] bg-fg" />
        </div>
      </div>
      <Overview />
    </div>
  );
});

// -- the settings ------------------------------------------------------------------

/**
 * The top of OutputInspector on a phone: the Clip | Output tabs and the first
 * row of the Clip tab, the aspect ratio at its default. Still; the film does
 * not touch them.
 */
const Inspector = memo(function Inspector({ strings }: { strings: IntroStrings }) {
  return (
    <div className="min-h-0 flex-1 border-t border-[var(--border)] pt-2">
      <div className="border-b border-transparent px-4 pb-3 pt-1">
        <Segmented options={[{ label: strings.clipTab }, { label: strings.outputTab }]} thumb={0} width={TRACK_W} />
      </div>
      <div className="px-4 pt-1">
        <div className="overflow-hidden rounded-[var(--radius-card)] border border-card-edge bg-surface">
          <div className="py-1.5 pl-4 pr-1.5">
            <div className="flex min-h-9 items-center justify-between gap-2">
              <span className="min-w-0 truncate text-[15px] text-fg">{strings.aspect}</span>
              <div className="w-[152px] shrink-0">
                <div className="flex h-12 w-full items-center gap-2 rounded-[var(--radius-control)] bg-fill px-3">
                  <span className="min-w-0 flex-1 truncate text-[15px] text-fg">{strings.original}</span>
                  <ChevronDown size={15} className="shrink-0 text-fg-muted" />
                </div>
              </div>
            </div>
          </div>
        </div>
      </div>
    </div>
  );
});

// -- the screen ----------------------------------------------------------------------

/**
 * The editor with the sample clip open, at film frame `f`. Draws the screen
 * from the top of the content to the gesture strip; the scene around it
 * steps the tab bar away and moves the camera.
 */
export function EditorScreen({ f, strings }: { f: number; strings: IntroStrings }) {
  const edit = editFrame(f);
  return (
    <div
      className="absolute inset-x-0 flex flex-col"
      style={{ top: CONTENT.top, bottom: SCREEN.height - GESTURE.top }}
    >
      <Bar
        f={Math.min(Math.max(f, REMOVE - 1), REMOVE + 10)}
        undo={ramp(f, SPLIT_LATE, 4)}
        strings={strings}
      />
      <Preview at={playheadAt(edit)} />
      <Transport f={edit} />
      <Timeline f={edit} />
      <Inspector strings={strings} />
    </div>
  );
}

// -- nothing open yet ------------------------------------------------------------------

/**
 * 'Video seç' is pressed, on the film's clock: some nine frames after the
 * card has come to rest and four before the editor comes, so the step reads
 * as a choice being made rather than a card flashing past.
 */
const CHOOSE = 555;

/**
 * The phone editor with nothing open (EditorPage's empty branch): the card
 * that asks for a video, which is what the Düzenle tab really opens --
 * nothing in İndirmeler opens a finished file here, so the film picks it, as
 * the user will. At film frame `f`; only the press moves, so the card is
 * drawn for a frame held around it.
 */
export function EditorPickScreen({ f, strings }: { f: number; strings: IntroStrings }) {
  return <PickCard f={Math.min(Math.max(f, CHOOSE - 1), CHOOSE + 16)} strings={strings} />;
}

const PickCard = memo(function PickCard({ f, strings }: { f: number; strings: IntroStrings }) {
  return (
    <div className="absolute inset-x-0 px-4 pt-2" style={{ top: CONTENT.top }}>
      <div className="flex flex-col items-center rounded-[var(--radius-card)] border border-card-edge bg-surface px-6 py-9 text-center">
        <FilePlus2 size={28} strokeWidth={1.5} className="text-fg-faint" />
        <p className="mt-3 text-[14px] font-medium text-fg">{strings.pickTitle}</p>
        <p className="mt-0.5 text-[12.5px] text-fg-muted">{strings.pickBody}</p>
        <div className="mt-4 flex items-center gap-2">
          <ChooseButton f={f} label={strings.chooseVideo} />
          <FromLink label={strings.fromLink} />
        </div>
      </div>
    </div>
  );
});

/** Video seç (Button sm secondary): it gives, takes its pressed fill and ripples. */
function ChooseButton({ f, label }: { f: number; label: string }) {
  const pressing = press(f, CHOOSE);
  const held = (1 - pressing) / 0.04;
  return (
    <span
      className="relative inline-flex h-10 items-center justify-center overflow-hidden rounded-[var(--radius-control)] bg-fill px-3.5 text-[13.5px] font-medium text-fg"
      style={{ transform: pressing < 1 ? `scale(${pressing})` : undefined }}
    >
      {held > 0.001 && <span className="absolute inset-0 bg-fill-active" style={{ opacity: held }} />}
      <Ripple frame={f} at={CHOOSE} size={130} />
      <span className="relative">{label}</span>
    </span>
  );
}

/** Bağlantıdan al (Button sm ghost, with its link icon). */
const FromLink = memo(function FromLink({ label }: { label: string }) {
  return (
    <span className="inline-flex h-10 items-center justify-center gap-1.5 rounded-[var(--radius-control)] px-3.5 text-[13.5px] font-medium text-fg-muted">
      <Link2 size={15} className="block shrink-0" />
      {label}
    </span>
  );
});

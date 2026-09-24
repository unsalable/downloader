import { FolderOpen, Pause, X } from 'lucide-react';
import { memo } from 'react';
import { interpolate } from 'remotion';

import { ROW_LINE } from '@/components/ui/ListGroup';
import { cn } from '@/lib/cn';
import { formatBytes, formatEta, formatSpeed } from '@/lib/format';
import { EASE, mix, ramp, Segmented, SPRING, springFrom } from '../kit';
import { Landscape } from '../screens';
import type { IntroStrings } from '../strings';

/*
 * İndirmeler on the film's phone (S6): the Sürüyor half, with the one
 * download İndir just started running through the whole of its life -- live
 * progress, the merge of the video and audio streams, the finished file.
 *
 * Drawn from the real screen's classes (DownloadsPage, DownloadCard, Progress,
 * the phone's Segmented), with the moving parts taken from the frame. Only the
 * content: the phone scene brings the camera, the screen change, the tab bar
 * and the caption. Everything is placed in the screen's u, at the storyboard's
 * own numbers, so the scene's camera keys land where it says.
 *
 * The phone's "Pause all" above the list is left out: the row is the subject,
 * and a second pause button would only be something more to read.
 */

/**
 * İndirmeler is coming in (the phone scene changes to it at 414): the page
 * is drawn from here and its ticks are counted from here. The row has no
 * arrival of its own. The real list is only mounted once it has a row, and
 * its AnimatePresence does not play for what it starts with, so card and row
 * come in together, with the screen.
 */
const ARRIVE = 418;
/** The transfer is done and the streams are being merged. */
const MERGE = 465;
/**
 * The file is written: eighteen frames after the merge starts, long enough
 * to read 'Akışlar birleştiriliyor' -- the one hint that the best quality
 * comes from joining two streams.
 */
const DONE = 483;

/** The transfer's percent at its ticks, straight between them. */
const PERCENT_FRAMES = [434, 440, 447, 454, 460, 464];
const PERCENT_VALUES = [2, 21, 48, 79, 96, 100];

/** The first tick the row shows, from which the speed's changes are counted. */
const FIRST_TICK = ARRIVE - (ARRIVE % 3);

const MIB = 1024 * 1024;
/** What the network gives, a little different each time it is measured. */
const SPEEDS = [9.4, 10.1, 9.8].map((mib) => mib * MIB);
/** The plan line's 1080p MKV (S5), so the size promised is the size written. */
const SIZE = 115_343_360;
/** The quality and container as the backend labels them (plan.rs), as Home's plan line promised. */
const FORMAT_LABEL = '1080p - MKV';

/**
 * The indeterminate bar's pass: ud-sweep's 1.25 s. The merge is on screen for
 * about half a pass, so the pass is already a quarter along when it starts:
 * what is seen is the segment crossing the bar from one end to the other,
 * through the fast middle of its curve, not creeping in at an edge.
 */
const SWEEP_PASS = 37.5;
const SWEEP_LEAD = 10;

const LINE = cn(ROW_LINE, 'gap-x-3');

function percentAt(frame: number): number {
  if (frame < PERCENT_FRAMES[0]!) return 0;
  return interpolate(frame, PERCENT_FRAMES, PERCENT_VALUES, {
    extrapolateLeft: 'clamp',
    extrapolateRight: 'clamp',
  });
}

/**
 * One thing giving way to the next in the same place over `length` frames.
 * The old one is gone by half-way and the new one starts a frame before that:
 * two lines of different length laid over each other read as a smudge, so
 * they share at most a frame, and faintly.
 */
function crossfade(frame: number, at: number, length: number) {
  const half = length / 2;
  return {
    old: 1 - ramp(frame, at, half, EASE.in),
    next: ramp(frame, at + half - 1, length - half + 1),
  };
}

/**
 * The screen at film frame `f`. Nothing on it moves before it comes in or
 * once the folder has popped in, so it is drawn for a frame held inside that
 * stretch: mounted with the scene and hidden until it is wanted, it is not
 * drawn again on every frame of the film.
 */
export function DownloadsScreen({ f, strings }: { f: number; strings: IntroStrings }) {
  return <DownloadsPage f={Math.min(Math.max(f, ARRIVE - 1), DONE + 16)} strings={strings} />;
}

const DownloadsPage = memo(function DownloadsPage({ f, strings }: { f: number; strings: IntroStrings }) {
  // The bar follows the transfer every frame, as the real one's linear
  // transition draws a line between ticks; the numbers under it change only
  // on a tick, every third frame -- ten times a second, like the backend's.
  // The speed moves on every third tick, and the time left is reckoned at
  // the average rate, so it counts down steadily instead of jumping about.
  const percent = percentAt(f);
  const tick = f - (f % 3);
  const ticked = percentAt(tick);
  const speed = SPEEDS[Math.max(0, Math.floor((tick - FIRST_TICK) / 9)) % SPEEDS.length]!;
  const eta = Math.round(((100 - ticked) / 100) * (SIZE / MIB / 9.8));

  const toMerge = crossfade(f, MERGE, 6);
  const toDone = crossfade(f, DONE, 8);

  return (
    <div className="absolute inset-x-4" style={{ top: 60 }}>
      <Halves active={strings.downloadsActive} history={strings.navHistory} />

      <div className="mt-4 overflow-hidden rounded-[var(--radius-card)] border border-card-edge bg-surface">
        <div className="flex items-center pr-1.5">
          <div className="flex min-w-0 flex-1 items-center gap-3 py-2.5 pl-3 pr-2">
            <Thumbnail />
            <div className="block min-w-0 flex-1">
              <Title text={strings.sampleTitle} />
              <Bar frame={f} percent={percent} />
              <Readout
                percent={Math.floor(ticked)}
                speed={formatSpeed(speed)}
                eta={formatEta(eta)}
                live={toMerge.old}
                merging={f < DONE ? toMerge.next : toDone.old}
                done={toDone.next}
                mergingLabel={strings.merging}
              />
            </div>
          </div>

          <div className="flex shrink-0 items-center gap-0.5">
            <PauseToFolder frame={f} />
            <Cancel />
          </div>
        </div>
      </div>
    </div>
  );
});

/** Sürüyor | Geçmiş, on Sürüyor: the half a download that was just started opens. */
const Halves = memo(function Halves({ active, history }: { active: string; history: string }) {
  return <Segmented options={[{ label: active }, { label: history }]} thumb={0} width={358} />;
});

const Thumbnail = memo(function Thumbnail() {
  return (
    <div className="flex h-11 w-[72px] shrink-0 overflow-hidden rounded-[var(--radius-thumb)] bg-surface-sunken">
      <Landscape style={{ width: '100%', height: '100%' }} />
    </div>
  );
});

const Title = memo(function Title({ text }: { text: string }) {
  return (
    <span className="block truncate text-[13.5px] font-medium leading-[18px] text-fg">{text}</span>
  );
});

/**
 * The progress bar (ui/Progress): the fill scaled from its left edge while the
 * transfer runs, then the indeterminate segment while the streams are merged,
 * then gone. Its room is kept after it goes, so the line under it stays put.
 */
function Bar({ frame, percent }: { frame: number; percent: number }) {
  const gone = ramp(frame, DONE, 5, EASE.in);
  if (gone >= 1) return <div className="mt-1.5 h-[3px]" />;

  // The full fill hands over to the sweeping segment in the same colour, so
  // the bar reads as draining into it rather than blinking empty.
  const sweeping = ramp(frame, MERGE, 4);
  const pass = (((frame - MERGE + SWEEP_LEAD) % SWEEP_PASS) + SWEEP_PASS) % SWEEP_PASS;
  const along = mix(-100, 250, EASE.inOut(pass / SWEEP_PASS));

  return (
    <div
      className="relative mt-1.5 h-[3px] w-full overflow-hidden rounded-full bg-fill-hover"
      style={{ opacity: 1 - gone }}
    >
      {sweeping < 1 && (
        <div
          className="h-full w-full origin-left bg-accent"
          style={{ opacity: 1 - sweeping, transform: `scaleX(${percent / 100})` }}
        />
      )}
      {sweeping > 0 && (
        <div
          className="absolute inset-y-0 left-0 w-2/5 rounded-full bg-accent"
          style={{ opacity: sweeping, transform: `translateX(${along}%)` }}
        />
      )}
    </div>
  );
}

interface ReadoutProps {
  percent: number;
  speed: string;
  eta: string;
  /** How much of each of the three lines is showing. */
  live: number;
  merging: number;
  done: number;
  mergingLabel: string;
}

/**
 * The line under the title, in the three forms it takes, laid over one
 * another in one place. Memoised: its values change on a tick, not on every
 * frame, and between ticks it is left alone.
 */
const Readout = memo(function Readout({ percent, speed, eta, live, merging, done, mergingLabel }: ReadoutProps) {
  return (
    <span className="mt-1 grid">
      {live > 0 && (
        <span className={LINE} style={{ gridArea: '1 / 1', opacity: live }}>
          {/* Held to a width, as in the app, so what follows does not shuffle
              sideways when a digit is added. */}
          <span className="min-w-[34px]">{percent}%</span>
          <span>{speed}</span>
          <span>{eta}</span>
        </span>
      )}
      {merging > 0 && (
        <span className={LINE} style={{ gridArea: '1 / 1', opacity: merging }}>
          <span className="min-w-0 max-w-full truncate">{mergingLabel}</span>
        </span>
      )}
      {done > 0 && (
        <span className={LINE} style={{ gridArea: '1 / 1', opacity: done }}>
          <span className="min-w-0 max-w-full truncate">{FORMAT_LABEL}</span>
          <span>{formatBytes(SIZE)}</span>
        </span>
      )}
    </span>
  );
});

/** The first action: Pause while it runs, then the folder the file is in, arriving with a small pop. */
function PauseToFolder({ frame }: { frame: number }) {
  const swap = crossfade(frame, DONE, 6);
  // Sprung from the frame the folder starts to show, or the pop would be
  // spent before it could be seen.
  const pop = mix(0.8, 1, springFrom(frame, DONE + 2, SPRING.snap));
  return (
    <span className="grid size-9 shrink-0 place-items-center rounded-[8px] text-fg-muted">
      {swap.old > 0 && (
        <Pause size={15} style={{ gridArea: '1 / 1', opacity: swap.old }} />
      )}
      {swap.next > 0 && (
        <FolderOpen
          size={15}
          style={{ gridArea: '1 / 1', opacity: swap.next, transform: pop < 1 ? `scale(${pop})` : undefined }}
        />
      )}
    </span>
  );
}

/** Cancel, which stays Remove once the file is written: the same X. */
const Cancel = memo(function Cancel() {
  return (
    <span className="grid size-9 shrink-0 place-items-center rounded-[8px] text-fg-muted">
      <X size={15} />
    </span>
  );
});

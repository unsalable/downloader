import { memo, useEffect, useRef, useState, type RefObject } from 'react';

import { useTranslation } from '@/i18n';
import { cn } from '@/lib/cn';
import { IS_MOBILE } from '@/lib/platform';
import type { AspectRatio, FrameFit } from '@/types';

/*
 * The picture, with the export's frame drawn over it.
 *
 * It is not a player: it has no controls attribute and no transport of its
 * own, because the page owns playback and two sets of controls that disagree
 * with each other are worse than one. It is not a status panel either -- a
 * file the window cannot decode still shows its frame and its cuts, and the
 * editor loses the moving picture and nothing else.
 *
 * What it does own is the arithmetic. Where the picture lands inside the stage
 * and where the chosen aspect lands inside the picture are two nested
 * letterboxes, and neither is expressible in CSS from the source's dimensions
 * alone, which is why the stage is measured and both rectangles are laid out
 * in pixels here.
 */

/** The chosen shapes as numbers. `source` has none: it is the absence of one. */
const RATIOS: Record<Exclude<AspectRatio, 'source'>, number> = {
  '16:9': 16 / 9,
  '9:16': 9 / 16,
  '16:10': 16 / 10,
  '4:3': 4 / 3,
  '1:1': 1,
};

/**
 * Below this the two rectangles are the same rectangle. A 16:9 clip asked for
 * 16:9 loses nothing, and rounding is the only thing that says otherwise.
 */
const EPSILON_PX = 0.5;

/*
 * Playing at the export's level.
 *
 * Up to 100 % the element's own volume does it. Past that only Web Audio can:
 * the element is handed to a graph with a gain in it. That is a one-way trip --
 * an element given to `createMediaElementSource` never plays through its own
 * output again -- and for media from another origin without CORS approval the
 * graph hears nothing but silence, for good.
 *
 * The picture comes from Tauri's asset protocol, which is another origin than
 * the page (http://asset.localhost against http://tauri.localhost). Tauri
 * answers every one of those requests with `Access-Control-Allow-Origin` set to
 * the page's own origin (protocol/asset.rs in the tauri crate), so the element
 * asks in CORS mode and the approval is there to be had. Even so it is only
 * handed over once it has actually loaded that way, and only when a level past
 * 100 % is asked for: a preview that never goes above 100 % never touches Web
 * Audio at all. If a load in CORS mode is ever refused, the element asks again
 * without it and the preview simply stops at 100 % -- the picture matters more
 * than the last half of a volume slider.
 */

/** One gain per element, because an element can only be handed over once. */
const amplifiers = new WeakMap<HTMLMediaElement, GainNode>();

/** Shared: every element's graph can run on the same clock. */
let audioContext: AudioContext | null = null;

/**
 * Let the clock stop. A running context keeps the audio output awake whether
 * or not anything is playing through it -- on a phone, a steady drain on the
 * battery for a paused preview -- so it is suspended whenever the picture
 * stops, and resumed the moment it plays (see `apply` below). Only one preview
 * is ever on screen, so the shared clock stopping never silences another.
 */
function rest() {
  if (audioContext?.state === 'running') void audioContext.suspend().catch(() => {});
}

function amplifierFor(video: HTMLMediaElement): GainNode | null {
  const existing = amplifiers.get(video);
  if (existing) return existing;
  try {
    audioContext ??= new AudioContext();
    const source = audioContext.createMediaElementSource(video);
    const gain = audioContext.createGain();
    source.connect(gain).connect(audioContext.destination);
    amplifiers.set(video, gain);
    return gain;
  } catch {
    return null;
  }
}

interface PreviewStageProps {
  src: string | null;
  /** Owned by the page, which drives playback through it. */
  videoRef: RefObject<HTMLVideoElement | null>;
  aspect: AspectRatio;
  fit: FrameFit;
  sourceWidth: number | null;
  sourceHeight: number | null;
  /** Linear gain, as the export will apply it: 0 is silence, 2 is 200 %. */
  volume: number;
  playable: boolean;
  onUnplayable: () => void;
  /** Told whether a level above 100 % can be heard here, or only in the export. */
  onCappedChange?: (capped: boolean) => void;
  /** A data: URI, for files the window cannot decode. */
  fallbackFrame?: string | null;
  className?: string;
}

interface Rect {
  width: number;
  height: number;
}

/**
 * Memoised: the page hands it the same numbers and the same callbacks until one
 * of them really changes, and it has a video element and two measured
 * rectangles that nothing else on the page should be able to disturb.
 */
export const PreviewStage = memo(function PreviewStage({
  src,
  videoRef,
  aspect,
  fit,
  sourceWidth,
  sourceHeight,
  volume,
  playable,
  onUnplayable,
  onCappedChange,
  fallbackFrame = null,
  className,
}: PreviewStageProps) {
  const { t } = useTranslation();
  const stageRef = useRef<HTMLDivElement>(null);
  const [stage, setStage] = useState<Rect | null>(null);
  // Whether the element asks for its media in CORS mode, which is what lets it
  // be amplified. Dropped for a source the first time a load of it that way is
  // refused, and asked for again by the next source: a file the window cannot
  // decode is refused in exactly the same way, and one of those must not cap
  // every clip opened after it.
  const [cors, setCors] = useState(true);
  const [corsSrc, setCorsSrc] = useState(src);
  if (corsSrc !== src) {
    setCorsSrc(src);
    setCors(true);
  }
  const [amplifierFailed, setAmplifierFailed] = useState(false);

  // Nothing is heard from a preview that cannot play, capped or not.
  const capped = playable && (!cors || amplifierFailed);
  useEffect(() => {
    onCappedChange?.(capped);
  }, [capped, onCappedChange]);

  // The same element, asked again without CORS. Changing the attribute alone
  // does not reload it; the media element only reads it when it starts over.
  useEffect(() => {
    if (!cors) videoRef.current?.load();
  }, [cors, videoRef]);

  // The level, applied to whichever element is on screen now. It runs again
  // when the element loads (only then can it be handed over) and whenever its
  // volume or mute changes -- M in the page mutes the element itself, and an
  // amplified element's gain has to follow that as well.
  useEffect(() => {
    const video = videoRef.current;
    if (!video) return;
    const apply = () => {
      let gain = amplifiers.get(video) ?? null;
      if (
        !gain &&
        volume > 1 &&
        video.crossOrigin === 'anonymous' &&
        video.readyState >= HTMLMediaElement.HAVE_METADATA
      ) {
        gain = amplifierFor(video);
        if (!gain) setAmplifierFailed(true);
      }
      if (gain) {
        // All of the level in the gain and none in the element, so the sum is
        // right whether or not the browser applies the element's volume before
        // the graph. Only written when it differs: each write that changes it
        // fires `volumechange`, which is what calls this.
        if (video.volume !== 1) video.volume = 1;
        gain.gain.value = video.muted ? 0 : volume;
        // Running only while there is something to hear. A context made or
        // left without a gesture waits, silently, until this resumes it.
        //
        // Starting the clock takes a moment, and it reads as suspended until
        // it has: a pause landing in that moment -- the end of the kept film
        // reached at once, a second tap -- found nothing to stop, and the
        // clock then ran on under a paused picture. So it is asked again once
        // it is running.
        if (video.paused) rest();
        else if (audioContext?.state === 'suspended') {
          void audioContext
            .resume()
            .then(() => {
              if (video.paused) rest();
            })
            .catch(() => {});
        }
      } else {
        const level = Math.min(1, Math.max(0, volume));
        if (video.volume !== level) video.volume = level;
      }
    };
    // Stopping is the amplified element's business alone: an element playing
    // at its own volume has no graph for the clock to be running.
    const pause = () => {
      if (amplifiers.has(video)) rest();
    };
    apply();
    video.addEventListener('loadedmetadata', apply);
    video.addEventListener('volumechange', apply);
    video.addEventListener('play', apply);
    video.addEventListener('pause', pause);
    return () => {
      video.removeEventListener('loadedmetadata', apply);
      video.removeEventListener('volumechange', apply);
      video.removeEventListener('play', apply);
      video.removeEventListener('pause', pause);
      // An element that has left the page takes its graph with it, and the
      // clock stops with nothing left to drive.
      if (!video.isConnected) {
        amplifiers.get(video)?.disconnect();
        amplifiers.delete(video);
        rest();
      }
    };
  }, [volume, videoRef, src, playable]);

  // The stage is whatever the page's layout leaves it, and both rectangles are
  // derived from it, so it is measured rather than assumed.
  useEffect(() => {
    const node = stageRef.current;
    if (!node) return;
    const observer = new ResizeObserver((entries) => {
      const measured = entries[0]?.contentRect;
      if (measured) setStage({ width: measured.width, height: measured.height });
    });
    observer.observe(node);
    return () => observer.disconnect();
  }, []);

  const layout = computeLayout({ stage, sourceWidth, sourceHeight, aspect, fit });

  // Without the source's dimensions there is no frame to draw and no picture
  // box to place, so the media falls back to filling the stage on its own.
  const boxStyle = layout
    ? {
        width: `${layout.picture.width}px`,
        height: `${layout.picture.height}px`,
      }
    : undefined;

  return (
    <div className={cn('flex min-h-0 flex-col', className)}>
      <div
        ref={stageRef}
        className={cn(
          'relative flex min-h-0 flex-1 items-center justify-center overflow-hidden',
          'rounded-[var(--radius-card)] bg-bg-deep',
        )}
      >
        <div
          className={cn('relative', layout ? 'shrink-0' : 'absolute inset-0')}
          style={boxStyle}
        >
          {playable && src != null ? (
            <video
              ref={videoRef}
              src={src}
              crossOrigin={cors ? 'anonymous' : undefined}
              // The page owns transport. This element only shows frames.
              playsInline
              preload="metadata"
              className="h-full w-full object-contain"
              onError={(event) => {
                // Refused before a single byte was understood, while asking in
                // CORS mode: ask once more without it before calling the file
                // unplayable. An element already handed to Web Audio is past
                // that point -- without CORS its graph would only hear silence.
                const video = event.currentTarget;
                if (
                  cors &&
                  video.readyState === HTMLMediaElement.HAVE_NOTHING &&
                  !amplifiers.has(video)
                ) {
                  setCors(false);
                  return;
                }
                onUnplayable();
              }}
              onLoadedMetadata={(event) => {
                const video = event.currentTarget;
                // H.265 on a machine with no HEVC decoder raises no error at
                // all: it reports a correct duration, plays its audio, and
                // hands over a zero-width picture. This is the only place that
                // failure announces itself.
                if (video.videoWidth === 0) {
                  onUnplayable();
                  return;
                }
                // A phone's WebView decodes nothing of a paused video it was
                // only asked the size of, so a clip opened on one showed a
                // black stage until it was played or scrubbed. Asking for the
                // position it is already at is a seek all the same, and a seek
                // paints the frame there.
                if (IS_MOBILE && video.paused && video.currentTime === 0) video.currentTime = 0;
              }}
            />
          ) : fallbackFrame != null ? (
            <img
              src={fallbackFrame}
              alt=""
              className="no-drag h-full w-full object-contain"
            />
          ) : null}

          {/* What FILL crops away, covered rather than outlined: a clip with
              nothing reframed should look like a plain picture and not like a
              selection, and only the part being lost has anything to say. */}
          {layout?.reframed && fit === 'fill' && (
            <>
              {layout.frame.width < layout.picture.width - EPSILON_PX && (
                <>
                  <div
                    aria-hidden="true"
                    className="pointer-events-none absolute inset-y-0 left-0 bg-bg/60"
                    style={{ width: `${(layout.picture.width - layout.frame.width) / 2}px` }}
                  />
                  <div
                    aria-hidden="true"
                    className="pointer-events-none absolute inset-y-0 right-0 bg-bg/60"
                    style={{ width: `${(layout.picture.width - layout.frame.width) / 2}px` }}
                  />
                </>
              )}
              {layout.frame.height < layout.picture.height - EPSILON_PX && (
                <>
                  <div
                    aria-hidden="true"
                    className="pointer-events-none absolute inset-x-0 top-0 bg-bg/60"
                    style={{ height: `${(layout.picture.height - layout.frame.height) / 2}px` }}
                  />
                  <div
                    aria-hidden="true"
                    className="pointer-events-none absolute inset-x-0 bottom-0 bg-bg/60"
                    style={{ height: `${(layout.picture.height - layout.frame.height) / 2}px` }}
                  />
                </>
              )}
            </>
          )}
        </div>

        {/* The frame itself. In FIT mode nothing is cropped, so this is the
            rectangle the picture will sit inside and the gap around the
            picture is the black bars the export will write. It is centred on
            the stage in both modes, which is where both nested letterboxes put
            it, so it is positioned against the stage rather than the picture. */}
        {layout?.reframed && (
          <div
            aria-hidden="true"
            className={cn(
              'pointer-events-none absolute left-1/2 top-1/2 border border-border-strong',
              '-translate-x-1/2 -translate-y-1/2',
            )}
            style={{
              width: `${layout.frame.width}px`,
              height: `${layout.frame.height}px`,
            }}
          />
        )}
      </div>

      {!playable && (
        // Underneath, not instead of. The cuts still work, and a panel here
        // would turn a missing decoder into a degraded mode.
        <div className="mt-2 text-center">
          <p className="text-[12.5px] font-medium text-fg-muted">{t('editor.noPreview')}</p>
          <p className="text-[12.5px] leading-[1.4] text-fg-faint">{t('editor.noPreviewBody')}</p>
        </div>
      )}
    </div>
  );
});

interface LayoutInput {
  stage: Rect | null;
  sourceWidth: number | null;
  sourceHeight: number | null;
  aspect: AspectRatio;
  fit: FrameFit;
}

interface Layout {
  /** Where the moving picture goes, centred on the stage. */
  picture: Rect;
  /** Where the export's frame goes, centred on the stage. */
  frame: Rect;
  /** Whether the two differ, which is the only reason to draw anything. */
  reframed: boolean;
}

/**
 * The two rectangles, in stage pixels.
 *
 * The nesting is the other way round in each mode, which is the whole
 * difference between them: FILL pours the picture into the stage and cuts the
 * frame out of it, while FIT pours the frame into the stage and drops the
 * picture inside that. Computing both from the same `min` keeps the two modes
 * from drifting apart the way two separate branches would.
 */
function computeLayout({
  stage,
  sourceWidth,
  sourceHeight,
  aspect,
  fit,
}: LayoutInput): Layout | null {
  if (!stage || stage.width <= 0 || stage.height <= 0) return null;
  if (!sourceWidth || !sourceHeight || sourceWidth <= 0 || sourceHeight <= 0) return null;

  const sourceAspect = sourceWidth / sourceHeight;
  const targetAspect = aspect === 'source' ? sourceAspect : RATIOS[aspect];

  const contain = (width: number, height: number, ratio: number): Rect => {
    const fitted = Math.min(width, height * ratio);
    return { width: fitted, height: fitted / ratio };
  };

  let picture: Rect;
  let frame: Rect;
  if (fit === 'fill') {
    picture = contain(stage.width, stage.height, sourceAspect);
    frame = contain(picture.width, picture.height, targetAspect);
  } else {
    frame = contain(stage.width, stage.height, targetAspect);
    picture = contain(frame.width, frame.height, sourceAspect);
  }

  const reframed =
    Math.abs(picture.width - frame.width) > EPSILON_PX ||
    Math.abs(picture.height - frame.height) > EPSILON_PX;

  return { picture, frame, reframed };
}

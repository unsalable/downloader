import { ChevronDown, ClipboardPaste, CornerDownLeft, Download, Eye, Music, Video, X } from 'lucide-react';
import { memo, type ReactNode } from 'react';

import { formatBytes, formatCount, formatDuration } from '@/lib/format';
import { FEATURED_PLATFORMS, platformPresentation } from '@/lib/platforms';
import { EASE, PlatformTile, Ripple, SCREEN, SPRING, Segmented, Spinner, Swap, mix, press, ramp, springFrom } from '../kit';
import { Landscape } from '../screens';
import type { IntroStrings } from '../strings';

/*
 * Home on the film's phone (S4 paste, S5 choose): the headline over an empty
 * field, a link pasted in, the site recognised, the headline stepping out of
 * the way, what was found and how to fetch it, a change of mind about audio,
 * and İndir. Drawn in the screen's u from the real Home's classes
 * (components/home), with every moving part read off the film frame `f`.
 *
 * Placed by hand rather than left to the page's flex column, because the
 * camera is aimed at these numbers; the gaps between them are still the real
 * page's (mt-3, mt-4, mt-5, gap-4), so the picture is the same. The collapse
 * is a translate, not the real height animation, so no frame lays out again.
 */

/** When each thing happens, on the film's clock. */
const AT = {
  /** Yapıştır is pressed. */
  paste: 306,
  /** The link is in the field; Analiz et takes the button's place. */
  link: 309,
  /** The site is recognised: the YouTube chip takes the tiles' row. */
  chip: 312,
  /** The headline steps out of the way and the field goes to the top. */
  collapse: 316,
  /**
   * The analysis is back: the preview and the options rise, as the field
   * and the camera come to rest.
   */
  found: 330,
  /** Yalnızca ses is pressed; the values follow two frames later. */
  audio: 370,
  /**
   * Video again, 22 frames on: the audio state is held long enough to read
   * what it changed, not only to see the thumb go over and back.
   */
  video: 392,
  /** İndir. */
  download: 408,
} as const;

/** The page's column: px-6 on a 390 screen. */
const COLUMN = { left: 24, width: 342 } as const;

/**
 * Where things sit before the collapse, in u. The headline hangs from the
 * field, mb-8 above it as on the page, so a title that fits on one line (the
 * Turkish) and one that takes two (the English) both leave the page's gap.
 */
const FIELD_TOP = 387;
const HERO_BOTTOM = FIELD_TOP - 32;
const HINT_TOP = 499;
/**
 * How far the field and the row under it travel up. The field lands at 56,
 * 12 under the content's top; the result starts mt-5 under the row (172), so
 * the options panel ends just clear of the tab bar at 758.
 */
const LIFT = 331;
const RESULT_TOP = 172;

/** The options panel's inner width: the column less its hairline and p-4. */
const PANEL_INNER = COLUMN.width - 2 - 32;

/**
 * What goes in the field. Never a real video's address, and it does not
 * matter what the id is: the field is too narrow to show it.
 */
const LINK = 'https://www.youtube.com/watch?v=0000000000';

/** The sample's facts, formatted by the app's own formatters. */
const SECONDS = 272;
const LENGTH = formatDuration(SECONDS);
const VIEWS = formatCount(1_240_000);
/**
 * What the plan line says in each mode, as the backend words its label. A
 * typical YouTube link's best streams are AVC video and Opus audio, which
 * the planner puts in MKV (plan.rs default_container, and its test); audio
 * alone keeps its source's WebM, at Opus's usual 160 kbps for the clip's
 * length.
 */
const PLAN = {
  video: { label: '1080p - MKV', size: formatBytes(115_343_360) },
  audio: { label: '160 kbps - WEBM', size: formatBytes((SECONDS * 160_000) / 8) },
} as const;

/** The featured row at rest: 28px tiles, 10px apart (PlatformIndicator). */
const FEATURED_WIDTH = FEATURED_PLATFORMS.length * 28 + (FEATURED_PLATFORMS.length - 1) * 10;

// -- still parts ----------------------------------------------------------

const Hero = memo(function Hero({ title, subtitle }: { title: string; subtitle: string }) {
  return (
    <div className="text-center">
      {/* Balanced: both titles fit on one line today, and one that wraps
          should break evenly rather than leave a word alone. */}
      <h2 className="text-balance text-[28px] font-semibold leading-[1.12] tracking-[-0.03em] text-fg">{title}</h2>
      <p className="mt-2.5 text-[14px] text-fg-muted">{subtitle}</p>
    </div>
  );
});

const ShareHint = memo(function ShareHint({ text }: { text: string }) {
  return <p className="text-center text-[12.5px] leading-relaxed text-fg-faint">{text}</p>;
});

const FeaturedTiles = memo(function FeaturedTiles() {
  return (
    <div className="flex w-full items-center justify-between" style={{ maxWidth: FEATURED_WIDTH }}>
      {FEATURED_PLATFORMS.map((id) => (
        <PlatformTile key={id} platform={id} size={28} />
      ))}
    </div>
  );
});

/** What the link was recognised as (PlatformIndicator, known platform). */
const YouTubeChip = memo(function YouTubeChip() {
  return (
    <div className="flex items-center gap-2 rounded-full bg-fill py-1 pl-1 pr-3">
      <PlatformTile platform="youtube" size={22} />
      <span className="text-[12.5px] font-medium text-fg">{platformPresentation('youtube').label}</span>
    </div>
  );
});

/** What was found (MediaPreviewCard): the picture and its length, then the facts. */
const PreviewCard = memo(function PreviewCard({ title, channel }: { title: string; channel: string }) {
  return (
    <article className="overflow-hidden rounded-[var(--radius-card)] border border-card-edge bg-surface">
      <div className="relative aspect-video w-full overflow-hidden bg-surface-sunken">
        <Landscape style={{ width: '100%', height: '100%' }} />
        <span className="tabular absolute bottom-2.5 right-2.5 flex items-center gap-1 rounded-md bg-black/70 px-1.5 py-0.5 text-[12px] font-medium text-white">
          {LENGTH}
        </span>
      </div>
      <div className="p-4">
        <h3 className="line-clamp-2 text-[15px] font-semibold leading-snug tracking-[-0.01em] text-fg">{title}</h3>
        <div className="mt-2 flex flex-wrap items-center gap-x-3 gap-y-1 text-[12.5px] text-fg-muted">
          <span className="flex items-center gap-1.5">
            <PlatformTile platform="youtube" size={22} />
            {platformPresentation('youtube').label}
          </span>
          <span className="truncate">{channel}</span>
          <span className="tabular flex items-center gap-1">
            <Eye size={13} />
            {VIEWS}
          </span>
        </div>
      </div>
    </article>
  );
});

/** A labelled dropdown (ui/Dropdown's trigger, at the phone's size), closed. */
function Choice({ label, children }: { label: string; children: ReactNode }) {
  return (
    <div className="flex min-w-0 flex-col gap-1.5">
      <span className="text-[12.5px] font-medium text-fg-muted">{label}</span>
      <div className="flex h-12 w-full items-center gap-2 rounded-[var(--radius-control)] bg-fill px-3">
        <span className="min-w-0 flex-1 truncate text-[15px] text-fg">{children}</span>
        <ChevronDown size={15} className="shrink-0 text-fg-muted" />
      </div>
    </div>
  );
}

const FormatChoice = memo(function FormatChoice({ label, value }: { label: string; value: string }) {
  return <Choice label={label}>{value}</Choice>;
});

function PlanLine({ plan }: { plan: (typeof PLAN)[keyof typeof PLAN] }) {
  return (
    <span className="flex flex-wrap items-center justify-center gap-x-3">
      <span>{plan.label}</span>
      <span className="tabular">~{plan.size}</span>
    </span>
  );
}

// -- moving parts ---------------------------------------------------------

/**
 * A value that goes over to `b` at `there` and comes back to `a` at `back`,
 * each time through the kit's Swap.
 */
function ThereAndBack({ f, a, b, there, back }: { f: number; a: ReactNode; b: ReactNode; there: number; back: number }) {
  return f < back ? <Swap frame={f} at={there} from={a} to={b} /> : <Swap frame={f} at={back} from={b} to={a} />;
}

/**
 * The field (UrlInput): the placeholder and the quiet Yapıştır button; once
 * pressed, the link and the accent Analiz et, disabled at 60% with its
 * spinner turning until the analysis is back. Only then does the clear button
 * come in, as the real one is hidden while the field is analysing.
 *
 * Both buttons are stacked in one cell from the start, so the cell is as wide
 * as the wider one throughout and nothing beside it moves when they trade.
 */
function UrlField({ f, strings }: { f: number; strings: IntroStrings }) {
  const pasted = f >= AT.link;
  const trade = ramp(f, AT.link, 5);
  const done = ramp(f, AT.found, 4);

  const pressing = press(f, AT.paste);
  const held = (1 - pressing) / 0.04;

  return (
    <div className="rounded-[var(--radius-card)] bg-surface shadow-[inset_0_0_0_1px_var(--card-edge)]">
      <div className="flex h-[52px] items-center gap-2 pl-4 pr-2">
        {pasted ? (
          <span className="min-w-0 flex-1 truncate text-[15px] text-fg" style={{ opacity: ramp(f, AT.link, 4) }}>
            {LINK}
          </span>
        ) : (
          <span className="min-w-0 flex-1 truncate text-[15px] text-fg-faint">{strings.placeholder}</span>
        )}

        {done > 0 && (
          <span
            className="flex shrink-0 rounded-full p-1.5 text-fg-faint"
            style={{ opacity: done, transform: done < 1 ? `scale(${mix(0.7, 1, done)})` : undefined }}
          >
            <X size={15} />
          </span>
        )}

        <span className="grid shrink-0 justify-items-end">
          <span
            className="relative flex h-9 items-center gap-1.5 overflow-hidden rounded-[var(--radius-control)] px-3 text-[13px] font-medium text-fg-muted"
            style={{
              gridArea: '1 / 1',
              opacity: 1 - ramp(f, AT.link, 5, EASE.in),
              visibility: trade < 1 ? undefined : 'hidden',
              transform: pressing < 1 ? `scale(${pressing})` : undefined,
            }}
          >
            {held > 0.001 && <span className="absolute inset-0 bg-fill-active" style={{ opacity: held }} />}
            <Ripple frame={f} at={AT.paste} size={112} />
            <ClipboardPaste size={15} className="relative" />
            <span className="relative">{strings.paste}</span>
          </span>
          <span
            className="flex h-9 items-center gap-1.5 rounded-[var(--radius-control)] bg-accent px-3.5 text-[13px] font-medium text-accent-fg"
            style={{ gridArea: '1 / 1', opacity: trade * mix(0.6, 1, done) }}
          >
            <span className="grid size-[14px]">
              {done < 1 && (
                <span style={{ gridArea: '1 / 1', opacity: 1 - done }}>
                  <Spinner frame={f} size={14} />
                </span>
              )}
              {done > 0 && (
                <span style={{ gridArea: '1 / 1', opacity: done }}>
                  <CornerDownLeft size={14} className="block" />
                </span>
              )}
            </span>
            {strings.analyze}
          </span>
        </span>
      </div>
    </div>
  );
}

/** The row under the field: the featured tiles, then what the link turned out to be. */
function PlatformRow({ f }: { f: number }) {
  const tilesOut = ramp(f, AT.link, 5, EASE.in);
  const chipIn = ramp(f, AT.chip, 8);
  return (
    <div className="relative mt-3 h-8">
      {tilesOut < 1 && (
        <div className="absolute inset-0 flex items-center justify-center" style={{ opacity: 1 - tilesOut }}>
          <FeaturedTiles />
        </div>
      )}
      {chipIn > 0 && (
        <div className="absolute inset-0 flex items-center justify-center" style={{ opacity: chipIn }}>
          <YouTubeChip />
        </div>
      )}
    </div>
  );
}

/** İndir (Button cta lg): it gives, darkens to the hover accent and ripples in the accent's ink. */
function DownloadButton({ f, label }: { f: number; label: string }) {
  const pressing = press(f, AT.download, 0.97);
  const held = (1 - pressing) / 0.03;
  return (
    <div
      className="relative mt-4 flex h-[52px] w-full items-center justify-center gap-2.5 overflow-hidden rounded-[var(--radius-card)] bg-accent px-6 text-[15px] font-medium text-accent-fg"
      style={{ transform: pressing < 1 ? `scale(${pressing})` : undefined }}
    >
      {held > 0.001 && <span className="absolute inset-0 bg-accent-hover" style={{ opacity: held }} />}
      <Ripple frame={f} at={AT.download} size={330} color="var(--accent-fg)" strength={0.18} />
      <Download size={17} className="relative shrink-0" />
      <span className="relative truncate">{label}</span>
    </div>
  );
}

/**
 * The options (DownloadOptionsPanel) with what the film needs and no more:
 * the mode, quality and format, İndir and the plan line under it. The save
 * folder and the advanced streams are left out on purpose.
 *
 * Switching to audio changes the quality and the plan, and not the format:
 * a mode change clears the container choice, so "keep the source's" is true
 * in both.
 */
function OptionsPanel({ f, strings }: { f: number; strings: IntroStrings }) {
  const thumb = springFrom(f, AT.audio, SPRING.glide) - springFrom(f, AT.video, SPRING.glide);
  return (
    <section className="rounded-[var(--radius-card)] border border-card-edge bg-surface p-4">
      <div className="mb-4 flex flex-col gap-1.5">
        <span className="text-[12.5px] font-medium text-fg-muted">{strings.mode}</span>
        <Segmented
          options={[
            { label: strings.modeVideo, icon: <Video size={14} /> },
            { label: strings.modeAudio, icon: <Music size={14} /> },
          ]}
          thumb={thumb}
          width={PANEL_INNER}
          pressed={[press(f, AT.video), press(f, AT.audio)]}
        />
      </div>

      <div className="grid grid-cols-2 gap-3">
        <Choice label={strings.quality}>
          <ThereAndBack f={f} a={strings.qualityBest} b={strings.audioBest} there={AT.audio + 2} back={AT.video + 2} />
        </Choice>
        <FormatChoice label={strings.format} value={strings.containerKeep} />
      </div>

      <DownloadButton f={f} label={strings.download} />

      <p className="mt-2.5 flex justify-center text-[12.5px] text-fg-muted">
        <ThereAndBack
          f={f}
          a={<PlanLine plan={PLAN.video} />}
          b={<PlanLine plan={PLAN.audio} />}
          there={AT.audio + 4}
          back={AT.video + 4}
        />
      </p>
    </section>
  );
}

// -- the screen -----------------------------------------------------------

/**
 * Home, in the screen's u, for film frame `f`. The scene around it moves the
 * screen as a whole (the camera, the change to İndirmeler); what moves here is
 * what moves on the page itself.
 *
 * Nothing on the page moves before Yapıştır is pressed or after İndir's
 * ripple has run out, so the page is drawn for a frame held inside that
 * stretch: while it waits, and once the scene has taken it away, it is not
 * drawn again on every frame of the film.
 */
export function HomeScreen({ f, strings }: { f: number; strings: IntroStrings }) {
  return <HomePage f={Math.min(Math.max(f, AT.paste - 1), AT.download + 12)} strings={strings} />;
}

const HomePage = memo(function HomePage({ f, strings }: { f: number; strings: IntroStrings }) {
  const heroOut = ramp(f, AT.collapse, 6, EASE.in);
  // The page goes up on the camera's curve and over the camera's frames (the
  // phone scene's move from P1 to P2), so the two travel as one and the field
  // settles where the camera does. On a curve of its own it shot up to the
  // top of the frame and the camera then dragged it back down.
  const lift = LIFT * ramp(f, AT.collapse, AT.found - AT.collapse, EASE.inOut);
  const raised = lift > 0 ? `translateY(${-lift}px)` : undefined;
  const found = ramp(f, AT.found, 8);

  return (
    <div className="absolute inset-0">
      {/* The headline rises with the field while it fades, as the real
          page's height collapse carries it, so the field never passes over
          it. */}
      {heroOut < 1 && (
        <div
          className="absolute"
          style={{
            left: COLUMN.left,
            bottom: SCREEN.height - HERO_BOTTOM,
            width: COLUMN.width,
            opacity: 1 - heroOut,
            transform: raised,
          }}
        >
          <Hero title={strings.heroTitle} subtitle={strings.heroSubtitle} />
        </div>
      )}

      <div
        className="absolute"
        style={{
          left: COLUMN.left,
          top: FIELD_TOP,
          width: COLUMN.width,
          transform: raised,
        }}
      >
        <UrlField f={f} strings={strings} />
        <PlatformRow f={f} />
      </div>

      {/* Shown only while the field is empty, as the real hint is: it goes
          when the link lands, with no exit of its own. */}
      {f < AT.link && (
        <div className="absolute" style={{ left: COLUMN.left, top: HINT_TOP, width: COLUMN.width }}>
          <ShareHint text={strings.shareHint} />
        </div>
      )}

      {/* Mounted from the start, unseen, so the page's largest block is not
          built on a frame the camera is moving. */}
      <div
        className="absolute flex flex-col gap-4"
        style={{
          left: COLUMN.left,
          top: RESULT_TOP,
          width: COLUMN.width,
          opacity: found,
          transform: found < 1 ? `translateY(${10 * (1 - found)}px)` : undefined,
        }}
      >
        <PreviewCard title={strings.sampleTitle} channel={strings.sampleChannel} />
        <OptionsPanel f={f} strings={strings} />
      </div>
    </div>
  );
});

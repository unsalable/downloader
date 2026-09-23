import { AnimatePresence, motion, useReducedMotionConfig } from 'motion/react';
import { open } from '@tauri-apps/plugin-dialog';
import { ChevronDown, RotateCcw, Sliders } from 'lucide-react';
import {
  memo,
  useEffect,
  useMemo,
  useRef,
  useState,
  type MouseEvent,
  type ReactNode,
} from 'react';

import { Button } from '@/components/ui/Button';
import { Dropdown, type DropdownOption } from '@/components/ui/Dropdown';
import { IconButton } from '@/components/ui/IconButton';
import { ListGroup, ListGroupLabel } from '@/components/ui/ListGroup';
import { Segmented } from '@/components/ui/Segmented';
import { ToggleRow } from '@/components/ui/SettingRow';
import { Slider } from '@/components/ui/Slider';
import { TextInput } from '@/components/ui/TextInput';
import { Toggle } from '@/components/ui/Toggle';
import { Tooltip } from '@/components/ui/Tooltip';
import { useTranslation, type TranslationKey } from '@/i18n';
import { cn } from '@/lib/cn';
import { estimatedBytes, formatMbps, parseMbps, sourceVideoKbps } from '@/lib/editor/bitrate';
import { basename, formatBytes, prettyCodec } from '@/lib/format';
import { COLLAPSE, SIDEWAYS, T } from '@/lib/motion';
import { IS_MOBILE } from '@/lib/platform';
import * as ipc from '@/services/ipc';
import {
  selectActiveClip,
  selectCuts,
  selectKeptLength,
  selectLosslessBlocker,
  selectOptions,
  useEditorStore,
  type LosslessBlocker,
} from '@/stores/useEditorStore';
import type {
  AspectRatio,
  AudioCodec,
  ExportMode,
  ExportQuality,
  FrameFit,
  Settings,
  VideoCodec,
} from '@/types';

/*
 * The editor's right-hand rail: two tabs, and everything the export can be
 * told.
 *
 * Clip holds what the preview can show -- the frame's shape and the sound's
 * level -- so every press there is answered on screen. Output holds what the
 * file is written as. One of them is watched at a time, and a rail this narrow
 * has room for one column of controls, not two.
 *
 * The tabs stay where they are and only what is under them scrolls. The rail
 * is exactly as tall as the stage beside it; a re-encode's settings are taller
 * than that, and before, they pushed the whole editor down instead of moving
 * inside their own column.
 *
 * What it deliberately is not: a place where a setting is accepted now and
 * refused at export time. Every combination the menus offer is one FFmpeg can
 * actually write -- a codec its container cannot hold is shown disabled with
 * its reason rather than hidden -- and a choice that rules out a lossless copy
 * moves the mode in the same step (see `withLegalMode`), with one line under
 * the mode saying which choice did it. It is also not a second store: the
 * pool, the cuts and the history live in `useEditorStore`, and this reads and
 * writes them directly rather than mirroring them into props.
 *
 * On a phone it is the lower half of the screen, full width, with the rows at
 * the size of the phone's other settings. Two things it offers a desktop are
 * not there: where the file is saved -- on Android an export lands in the
 * downloads folder and the gallery, and a folder picked there would come back
 * as a content address FFmpeg cannot write to -- and hardware encoding, which
 * no Android build of FFmpeg this app ships can do.
 */

interface OutputInspectorProps {
  /**
   * Only for the download folder: where the picker starts, and where a clip
   * of the app's own lands when no folder is chosen.
   */
  settings: Settings;
  /**
   * The preview cannot play louder than the file itself (see PreviewStage), so
   * a level above 100 % is only heard in the export.
   */
  previewCapped?: boolean;
  /**
   * The open clips, at the top of the Clip tab. A phone's, which has no rail
   * beside the picture to hold them.
   */
  clips?: ReactNode;
  className?: string;
}

type Tab = 'clip' | 'output';

/** The tabs in the order they stand, which is the way a switch between them moves. */
const TABS: Tab[] = ['clip', 'output'];

/** The quality menu's entries: the presets, and a rate of the user's own. */
type QualityChoice = ExportQuality | 'custom';

const ASPECTS: AspectRatio[] = ['source', '16:9', '9:16', '16:10', '4:3', '1:1'];

/**
 * The containers the app writes, and the codecs each can hold.
 *
 * This is the whole of the constraint the menus enforce: WebM is VP9 or AV1
 * with Opus, MOV is H.264 or H.265 with AAC, MP4 adds AV1 and MP3, and
 * Matroska holds whatever it is given.
 */
const CONTAINERS = ['mp4', 'mkv', 'webm', 'mov'] as const;

/** Format names as they are written, not as the file extension spells them. */
const CONTAINER_NAMES: Record<string, string> = {
  mp4: 'MP4',
  mkv: 'MKV',
  webm: 'WebM',
  mov: 'MOV',
};

const VIDEO_BY_CONTAINER: Record<string, VideoCodec[]> = {
  mp4: ['h264', 'h265', 'av1'],
  mkv: ['h264', 'h265', 'vp9', 'av1'],
  webm: ['vp9', 'av1'],
  mov: ['h264', 'h265'],
};

const AUDIO_BY_CONTAINER: Record<string, AudioCodec[]> = {
  mp4: ['aac', 'mp3'],
  mkv: ['aac', 'opus', 'mp3', 'flac'],
  webm: ['opus'],
  mov: ['aac'],
};

/**
 * The codecs in order of how alike they are, which is what "the nearest legal
 * one" is measured along. Ordered by generation rather than by name: dropped
 * into WebM, H.265 should land on VP9 and not on whatever happens to be first
 * in the list.
 */
const VIDEO_LADDER: VideoCodec[] = ['h264', 'h265', 'vp9', 'av1'];
const AUDIO_LADDER: AudioCodec[] = ['aac', 'mp3', 'opus', 'flac'];

const QUALITIES: ExportQuality[] = ['maximum', 'high', 'balanced', 'small'];

const QUALITY_LABELS: Record<ExportQuality, TranslationKey> = {
  maximum: 'editor.qualityMaximum',
  high: 'editor.qualityHigh',
  balanced: 'editor.qualityBalanced',
  small: 'editor.qualitySmall',
};

const HEIGHTS = [2160, 1440, 1080, 720, 480];
const RATES = [24, 30, 60];
const AUDIO_BITRATES = [320, 256, 192, 160, 128];

/** What `selectLosslessBlocker` names, as the row the user would go and undo. */
const BLOCKER_LABELS: Record<LosslessBlocker, TranslationKey> = {
  aspect: 'editor.aspect',
  fps: 'editor.fps',
  resolution: 'editor.resolution',
  volume: 'editor.volume',
  colour: 'editor.colour',
  container: 'editor.container',
};

/** Under this, a cut is where the user put it as far as anyone can tell. */
const SNAP_WORTH_SAYING_SEC = 0.1;

/**
 * The legal choice closest to the one the user had.
 *
 * Changing the container should move the codec as little as it can: the choice
 * is kept when the new container can hold it, and otherwise replaced by
 * whichever legal codec sits nearest along the ladder above.
 */
function nearest<T extends string>(ladder: readonly T[], legal: readonly T[], current: T): T {
  if (legal.includes(current)) return current;
  const from = ladder.indexOf(current);
  let best = legal[0]!;
  let distance = Number.POSITIVE_INFINITY;
  for (const candidate of legal) {
    const gap = Math.abs(ladder.indexOf(candidate) - from);
    if (gap < distance) {
      best = candidate;
      distance = gap;
    }
  }
  return best;
}

/**
 * Keep a pointer press on a switch or a segment from taking the keyboard.
 *
 * The editor's keys answer from whatever inside it has focus, and Chromium's
 * rule for the focus ring is that a key pressed while an element is focused
 * makes it `:focus-visible`. So a tab clicked with the
 * mouse lit up in orange on the next Space or arrow key -- a ring saying the
 * keyboard was on the tab while the key went to the transport. A press here
 * leaves focus where it was, which is also what the same controls do on a Mac.
 * Tab still reaches every one of them, ring and all. Menus and fields are left
 * alone: they need focus to be typed into.
 */
function keepFocus(event: MouseEvent) {
  const target = event.target as Element | null;
  if (target?.closest('[role="radio"], [role="switch"], [data-keep-focus]')) event.preventDefault();
}

/**
 * Memoised: it reads the store for itself, and the page above it re-renders
 * for its own reasons -- an export's progress, a play pressed -- none of which
 * change a setting.
 */
export const OutputInspector = memo(function OutputInspector({
  settings,
  previewCapped = false,
  clips,
  className,
}: OutputInspectorProps) {
  const { t, language } = useTranslation();
  const [tab, setTab] = useState<Tab>('clip');
  const [advanced, setAdvanced] = useState(false);
  const [scrolled, setScrolled] = useState(false);
  const scrollRef = useRef<HTMLDivElement>(null);
  // On a phone the new tab's content comes in from the side of the tab that
  // was picked, as Downloads' two halves do; a transform string is beyond the
  // reach of `MotionConfig`, so under the system's reduced motion it fades.
  const still = useReducedMotionConfig() ?? false;
  const direction = still ? 0 : tab === 'output' ? 1 : -1;

  const clip = useEditorStore(selectActiveClip);
  const options = useEditorStore(selectOptions);
  const blocker = useEditorStore(selectLosslessBlocker);
  const cuts = useEditorStore(selectCuts);
  const keptSec = useEditorStore(selectKeptLength);
  const outputDir = useEditorStore((state) => state.outputDir);
  const setOptions = useEditorStore((state) => state.setOptions);
  const setAspect = useEditorStore((state) => state.setAspect);
  const setOutputDir = useEditorStore((state) => state.setOutputDir);

  // Each tab opens at its top. The scroller outlives the switch, and landing
  // halfway down the other tab's column is landing somewhere arbitrary.
  useEffect(() => {
    scrollRef.current?.scrollTo({ top: 0 });
    setScrolled(false);
  }, [tab]);

  // How far a copied cut would really begin before the mark.
  //
  // A stream copy can only start at a keyframe, so the export quietly rolls
  // each start back to the one before it. The timeline already draws those
  // keyframes; this is the same fact as a number, next to the control that
  // decides whether it applies -- because finding it out from the length of
  // the finished file is not finding it out in time.
  const snappedBy = useMemo(() => {
    const marks = clip?.keyframes;
    if (options?.mode !== 'lossless' || !marks || marks.length === 0) return 0;
    let worst = 0;
    for (const cut of cuts) {
      let landing = 0;
      for (const mark of marks) {
        if (mark <= cut.startSec + 0.001) landing = mark;
        else break;
      }
      worst = Math.max(worst, cut.startSec - landing);
    }
    return worst;
  }, [clip?.keyframes, cuts, options?.mode]);

  // Where the file lands when no folder is chosen. Beside the source, as a
  // rule -- but a clip brought in from a link sits in the app's own temporary
  // folder, which is emptied, and its export goes to the download folder
  // instead. Only the backend knows which folders are the app's own, so it is
  // asked once per clip, and the row says what it answered. Kept with the path
  // it was asked about, so a clip never shows the answer for the one before.
  const clipPath = clip?.path ?? null;
  const [landing, setLanding] = useState<{ path: string; dir: string | null } | null>(null);
  useEffect(() => {
    if (IS_MOBILE || !clipPath) return;
    let live = true;
    void ipc
      .exportDefaultDir(clipPath)
      .catch(() => null)
      .then((dir) => {
        if (live) setLanding({ path: clipPath, dir });
      });
    return () => {
      live = false;
    };
    // The download folder is in the list because it is the answer: moved in
    // Settings, the row follows.
  }, [clipPath, settings.downloadDir]);

  // Nothing to inspect until a clip is open. The editor's empty state says so
  // in the middle of the screen; a rail of dead controls beside it would not.
  if (!clip || !options) return null;

  const hasAudio = clip.probe.hasAudio;
  const audible = hasAudio && !options.mute;
  const legalVideo = VIDEO_BY_CONTAINER[options.container] ?? VIDEO_LADDER;
  const legalAudio = AUDIO_BY_CONTAINER[options.container] ?? AUDIO_LADDER;
  const containerName = CONTAINER_NAMES[options.container] ?? options.container;

  // A change of format that takes the codecs with it; the store moves the
  // mode off lossless in the same step, because a stream copied out of one box
  // cannot be dropped into another.
  const chooseContainer = (container: string) => {
    setOptions({
      container,
      videoCodec: nearest(VIDEO_LADDER, VIDEO_BY_CONTAINER[container] ?? VIDEO_LADDER, options.videoCodec),
      audioCodec: nearest(AUDIO_LADDER, AUDIO_BY_CONTAINER[container] ?? AUDIO_LADDER, options.audioCodec),
    });
  };

  const pickFolder = async () => {
    const selected = await open({
      directory: true,
      multiple: false,
      defaultPath: outputDir ?? settings.downloadDir,
    });
    if (typeof selected === 'string') setOutputDir(selected);
  };

  // A rate above the source's would be invented frames, and a height above it
  // would be an upscale sold as a resolution. A file whose rate or height the
  // probe could not read offers everything rather than guessing a ceiling.
  const sourceFps = clip.probe.fps;
  const sourceHeight = clip.probe.height;
  const rates = RATES.filter((rate) => sourceFps == null || rate <= sourceFps + 0.5);
  const heights = HEIGHTS.filter((height) => sourceHeight == null || height <= sourceHeight);

  const quality: QualityChoice = options.videoBitrateKbps != null ? 'custom' : options.quality;
  const chooseQuality = (choice: QualityChoice) => {
    if (choice === quality) return;
    if (choice === 'custom') {
      // Starting from what the source was made at: the number people reach
      // for a custom rate to beat or to match, and one they can see is sane.
      setOptions({ videoBitrateKbps: sourceVideoKbps(clip.probe) });
    } else {
      setOptions({ quality: choice, videoBitrateKbps: null });
    }
  };

  // One line under the mode, and only one: what the mode costs, or -- when a
  // lossless copy would move a cut -- by how much, or what ruled it out.
  let modeLine: string;
  if (options.mode === 'lossless') {
    modeLine =
      snappedBy >= SNAP_WORTH_SAYING_SEC
        ? t('editor.losslessSnapped', {
            seconds: new Intl.NumberFormat(language, {
              minimumFractionDigits: 1,
              maximumFractionDigits: 1,
            }).format(snappedBy),
          })
        : t('editor.losslessHint');
  } else {
    modeLine =
      blocker != null
        ? t('editor.losslessRuledOut', { setting: t(BLOCKER_LABELS[blocker]) })
        : t('editor.reencodeHint');
  }

  const notIn = t('editor.notInContainer', { container: containerName });

  // Disabled and named, not hidden: a menu that quietly loses an entry when
  // the format changes teaches nothing about why it is gone. With a pointer
  // the reason is a hover away; a finger gets no hover, so on a phone it is
  // written under the entry instead.
  const codecOption = <C extends string>(codec: C, legal: readonly C[]): DropdownOption<C> => {
    const allowed = legal.includes(codec);
    return {
      value: codec,
      label: prettyCodec(codec) ?? codec,
      disabled: !allowed,
      disabledReason: allowed ? undefined : notIn,
      description: IS_MOBILE && !allowed ? notIn : undefined,
    };
  };
  const videoCodecOptions = VIDEO_LADDER.map((codec) => codecOption(codec, legalVideo));
  const audioCodecOptions = AUDIO_LADDER.map((codec) => codecOption(codec, legalAudio));

  const volumePercent = Math.round(options.volume * 100);

  // The folder the row names: the one chosen, or else the one the backend said
  // an export of this clip goes to instead of beside it. `undefined` while that
  // answer is on its way.
  const fallbackDir = landing?.path === clip.path ? landing.dir : undefined;
  const landsIn = outputDir ?? fallbackDir;

  const clipPane = (
    <>
      {clips && <div className="mb-5">{clips}</div>}

      <ListGroup>
        <Row
          title={t('editor.aspect')}
          control={
            // Six choices, one of them "Original": a segmented control would
            // give each about 35px and clip most of the labels. A menu spends
            // one row here and gives every choice a full line when open.
            <Picker
              value={options.aspect}
              options={ASPECTS.map((aspect) => ({
                value: aspect,
                label: aspect === 'source' ? t('editor.original') : aspect,
              }))}
              onChange={(aspect) => setAspect(aspect, options.fit)}
            />
          }
        >
          {/* How the picture meets a frame of another shape -- a question
              that only exists once there is another shape. */}
          <AnimatePresence initial={false}>
            {options.aspect !== 'source' && (
              <motion.div
                variants={COLLAPSE}
                initial="initial"
                animate="animate"
                exit="exit"
                className="overflow-hidden"
              >
                <div className="pb-1 pr-1.5 pt-2.5">
                  <Segmented
                    size="sm"
                    value={options.fit}
                    onChange={(fit: FrameFit) => setAspect(options.aspect, fit)}
                    options={[
                      { value: 'fill', label: t('editor.fill') },
                      { value: 'fit', label: t('editor.fit') },
                    ]}
                  />
                  <p className={cn('mt-1.5 leading-snug text-fg-muted', NOTE_TEXT)}>
                    {t(options.fit === 'fill' ? 'editor.fillHint' : 'editor.fitHint')}
                  </p>
                </div>
              </motion.div>
            )}
          </AnimatePresence>
        </Row>
      </ListGroup>

      <ListGroupLabel className="mt-5">{t('editor.audioTrack')}</ListGroupLabel>
      <ListGroup className="mt-1.5">
        <div
          // The slider's own label is this row's title, set at the size of
          // every other title in the rail rather than as a caption.
          className={cn(
            'px-4 pb-1 pt-3 [&_label]:font-normal [&_label]:text-fg',
            IS_MOBILE ? '[&_label]:text-[15px]' : '[&_label]:text-[13.5px]',
          )}
          // Back to the source's level in one gesture, the way a mixer's fader
          // resets: 100 % is the one value worth finding exactly.
          onDoubleClick={() => {
            if (audible) setOptions({ volume: 1 });
          }}
        >
          <Slider
            label={t('editor.volume')}
            value={volumePercent}
            min={0}
            max={200}
            step={5}
            disabled={!audible}
            formatValue={(value) => t('editor.volumeValue', { value })}
            onChange={(value) => setOptions({ volume: value / 100 })}
          />
        </div>
        <SwitchRow
          title={t('editor.muteAudio')}
          checked={options.mute}
          disabled={!hasAudio}
          onChange={(mute) => setOptions({ mute })}
        />
      </ListGroup>
      {!hasAudio ? (
        <Footnote>{t('editor.noAudio')}</Footnote>
      ) : (
        previewCapped &&
        audible &&
        volumePercent > 100 && <Footnote>{t('editor.volumePreviewCapped')}</Footnote>
      )}
    </>
  );

  const outputPane = (
    <>
      {/* The root of every other decision on this tab, and so the only
          control above the groups rather than inside one. */}
      <Segmented
        size="sm"
        value={options.mode}
        onChange={(mode: ExportMode) => setOptions({ mode })}
        options={[
          { value: 'lossless', label: t('editor.lossless'), disabled: blocker != null },
          { value: 'reencode', label: t('editor.reencode') },
        ]}
      />
      <p className={cn('mt-1.5 px-1 leading-snug text-fg-muted', NOTE_TEXT)}>{modeLine}</p>

      <ListGroup className="mt-4">
        <Row
          title={t('editor.container')}
          control={
            <Picker
              value={options.container}
              options={CONTAINERS.map((container) => ({
                value: container,
                label: CONTAINER_NAMES[container] ?? container,
              }))}
              onChange={chooseContainer}
            />
          }
        />
      </ListGroup>

      <AnimatePresence initial={false}>
        {options.mode === 'reencode' && (
          <motion.div
            variants={COLLAPSE}
            initial="initial"
            animate="animate"
            exit="exit"
            className="overflow-hidden"
          >
            <ListGroupLabel className="mt-5">{t('editor.videoGroup')}</ListGroupLabel>
            <ListGroup className="mt-1.5">
              <Row
                title={t('editor.codec')}
                control={
                  <Picker
                    value={options.videoCodec}
                    options={videoCodecOptions}
                    onChange={(videoCodec) => setOptions({ videoCodec })}
                  />
                }
              />
              <Row
                title={t('editor.quality')}
                control={
                  <Picker
                    value={quality}
                    options={[
                      ...QUALITIES.map((preset) => ({
                        value: preset as QualityChoice,
                        label: t(QUALITY_LABELS[preset]),
                      })),
                      { value: 'custom', label: t('editor.qualityCustom') },
                    ]}
                    onChange={chooseQuality}
                  />
                }
              >
                <AnimatePresence initial={false}>
                  {options.videoBitrateKbps != null && (
                    <motion.div
                      variants={COLLAPSE}
                      initial="initial"
                      animate="animate"
                      exit="exit"
                      className="overflow-hidden"
                    >
                      {/* The field and the estimate under it, which the
                          phone's editor keeps above the keyboard together. */}
                      <div data-keyboard-anchor className="pb-1 pr-1.5 pt-2.5">
                        <BitrateField
                          // A new clip is a new number to start from, not an
                          // edit of the last one's.
                          key={clip.id}
                          kbps={options.videoBitrateKbps}
                          audioKbps={audible ? (options.audioBitrateKbps ?? 0) : 0}
                          keptSec={keptSec}
                          onCommit={(videoBitrateKbps) => setOptions({ videoBitrateKbps })}
                        />
                      </div>
                    </motion.div>
                  )}
                </AnimatePresence>
              </Row>
              <Row
                title={t('editor.resolution')}
                control={
                  <Picker
                    value={options.maxHeight == null ? 'source' : String(options.maxHeight)}
                    options={[
                      { value: 'source', label: t('editor.original') },
                      ...heights.map((height) => ({ value: String(height), label: `${height}p` })),
                    ]}
                    onChange={(value) =>
                      setOptions({ maxHeight: value === 'source' ? null : Number(value) })
                    }
                  />
                }
              />
              <Row
                title={t('editor.fps')}
                control={
                  <Picker
                    value={options.fps == null ? 'source' : String(options.fps)}
                    options={[
                      { value: 'source', label: t('editor.original') },
                      ...rates.map((rate) => ({ value: String(rate), label: `${rate} fps` })),
                    ]}
                    onChange={(value) => setOptions({ fps: value === 'source' ? null : Number(value) })}
                  />
                }
              />
            </ListGroup>

            <ListGroupLabel className="mt-5">{t('editor.audioTrack')}</ListGroupLabel>
            <ListGroup className="mt-1.5">
              <Row
                title={t('editor.codec')}
                control={
                  <Picker
                    value={options.audioCodec}
                    options={audioCodecOptions}
                    onChange={(audioCodec) => setOptions({ audioCodec })}
                    // There is no codec to choose for a track that is being
                    // dropped, or that was never there.
                    disabled={!audible}
                  />
                }
              />
              <Row
                title={t('editor.bitrate')}
                control={
                  <Picker
                    value={options.audioBitrateKbps == null ? '' : String(options.audioBitrateKbps)}
                    options={AUDIO_BITRATES.map((kbps) => ({
                      value: String(kbps),
                      label: `${kbps} kbps`,
                    }))}
                    onChange={(value) => setOptions({ audioBitrateKbps: Number(value) })}
                    disabled={!audible}
                  />
                }
              />
            </ListGroup>

            <button
              type="button"
              data-keep-focus
              onClick={() => setAdvanced(!advanced)}
              aria-expanded={advanced}
              className={cn(
                'pressable -ml-1.5 flex items-center gap-1.5 rounded-md px-1.5 font-medium text-fg-muted hover:text-fg',
                // A line of text is too thin a target for a finger.
                IS_MOBILE ? 'mt-2 min-h-11 text-[14px]' : 'mt-3 py-1 text-[12.5px]',
              )}
            >
              <Sliders size={13} />
              {t('editor.advanced')}
              <ChevronDown
                size={13}
                className={cn(
                  'transition-transform duration-150 ease-out-quint',
                  advanced && 'rotate-180',
                )}
              />
            </button>

            <AnimatePresence initial={false}>
              {advanced && (
                <motion.div
                  variants={COLLAPSE}
                  initial="initial"
                  animate="animate"
                  exit="exit"
                  className="overflow-hidden"
                >
                  <ListGroup className="mt-2">
                    <SwitchRow
                      title={t('editor.toneMap')}
                      checked={options.toneMapSdr}
                      onChange={(toneMapSdr) => setOptions({ toneMapSdr })}
                    />
                    {!IS_MOBILE && (
                      <SwitchRow
                        title={t('editor.hardware')}
                        description={t('editor.hardwareHint')}
                        checked={options.hardware}
                        onChange={(hardware) => setOptions({ hardware })}
                      />
                    )}
                  </ListGroup>
                </motion.div>
              )}
            </AnimatePresence>
          </motion.div>
        )}
      </AnimatePresence>

      {/* Where it lands, as one row: the answer, and the way to change it.
          A phone's exports go to its downloads, which is not a choice. */}
      {!IS_MOBILE && (
        <ListGroup className="mt-5">
          <div className="flex min-h-[52px] items-center gap-1.5 py-2 pl-4 pr-2.5">
            <div className="min-w-0 flex-1">
              <p className="text-[13.5px] text-fg">{t('editor.exportFolder')}</p>
              {landsIn == null ? (
                // A blank line rather than a guess while the answer is on its
                // way, which is the height of the words that replace it.
                <p className="truncate text-[12.5px] text-fg-muted">
                  {landsIn === null ? t('editor.besideSource') : '\u00a0'}
                </p>
              ) : (
                // The folder's own name, which is what people recognise; the
                // path it sits on is a hover away. A path cut to fit this column
                // lost the one part worth reading.
                <Tooltip label={landsIn}>
                  <p className="truncate text-[12.5px] text-fg-muted">
                    {basename(landsIn.replace(/[\\/]+$/, '')) || landsIn}
                  </p>
                </Tooltip>
              )}
            </div>
            {outputDir != null && (
              <IconButton
                size="sm"
                icon={<RotateCcw size={14} />}
                label={t('convert.resetFolder')}
                onClick={() => setOutputDir(null)}
              />
            )}
            <Button size="sm" variant="secondary" onClick={() => void pickFolder()}>
              {t('options.change')}
            </Button>
          </div>
        </ListGroup>
      )}
    </>
  );

  const tabs = (
    <Segmented
      size="sm"
      value={tab}
      onChange={setTab}
      options={TABS.map((value) => ({
        value,
        label: t(value === 'clip' ? 'editor.clipTab' : 'editor.outputTab'),
      }))}
    />
  );

  return (
    <aside
      className={cn(
        'flex min-h-0 flex-col',
        !IS_MOBILE && 'w-[272px] shrink-0 xl:w-[300px]',
        className,
      )}
      onMouseDown={keepFocus}
    >
      {/* Pinned. A hairline appears under it once the column has scrolled,
          which is the only sign needed that there is more above. */}
      <div
        className={cn(
          'shrink-0 border-b transition-colors duration-150 ease-out-quint',
          IS_MOBILE ? 'px-4 pb-3 pt-1' : 'px-3 pb-3 pt-3',
          scrolled ? 'border-[var(--border)]' : 'border-transparent',
        )}
      >
        {tabs}
      </div>

      <div
        ref={scrollRef}
        onScroll={(event) => setScrolled(event.currentTarget.scrollTop > 0)}
        // The gutter is kept whether or not there is anything to scroll, so
        // switching to the taller tab does not shift every row sideways by
        // the width of the bar that appears. A phone's scrollbars float over
        // the content and take no room, so there it has no gutter to keep.
        className={cn(
          'min-h-0 flex-1 overflow-y-auto overscroll-contain',
          // A field brought into view above the keyboard stops short of its
          // edge rather than flush against it.
          IS_MOBILE
            ? 'overflow-x-hidden scroll-py-3 px-4 pb-6 pt-1'
            : 'pb-4 pl-3 pr-0.5 [scrollbar-gutter:stable]',
        )}
      >
        {IS_MOBILE ? (
          <AnimatePresence mode="wait" initial={false} custom={direction}>
            <motion.div
              key={tab}
              custom={direction}
              variants={SIDEWAYS}
              initial="initial"
              animate="animate"
              exit="exit"
            >
              {tab === 'clip' ? clipPane : outputPane}
            </motion.div>
          </AnimatePresence>
        ) : (
          <motion.div
            key={tab}
            initial={{ opacity: 0 }}
            animate={{ opacity: 1 }}
            transition={T.micro}
          >
            {tab === 'clip' ? clipPane : outputPane}
          </motion.div>
        )}
      </div>
    </aside>
  );
});

// -- rows ---------------------------------------------------------------------

/*
 * The rail's rows are its own rather than `SettingRow`'s. That row is built for
 * a settings page: its title keeps twelve rem to itself, which in a column this
 * narrow drops every switch and menu onto a line of its own and doubles the
 * height of everything. Here a title and its control share a line, the way an
 * inspector's do, and what belongs under a control (the frame's fit, a custom
 * rate) opens inside the same row rather than as a row of its own.
 *
 * A phone keeps that shape at its own sizes: fifteen-pixel titles, menus a
 * fingertip tall, and a control column wide enough for the longest choice in
 * either language -- "Özel bit hızı" -- beside the longest title at 360px.
 */

/** The size of a note under a control: a caption on the desktop, a line of text on a phone. */
const NOTE_TEXT = IS_MOBILE ? 'text-[13px]' : 'text-[12.5px]';

function Row({
  title,
  control,
  children,
}: {
  title: string;
  control: ReactNode;
  children?: ReactNode;
}) {
  return (
    <div className={IS_MOBILE ? 'py-1.5 pl-4 pr-1.5' : 'py-2 pl-4 pr-2.5'}>
      <div className="flex min-h-9 items-center justify-between gap-2">
        <span className={cn('min-w-0 truncate text-fg', IS_MOBILE ? 'text-[15px]' : 'text-[13.5px]')}>
          {title}
        </span>
        <div className={cn('shrink-0', IS_MOBILE ? 'w-[152px]' : 'w-[128px] xl:w-[148px]')}>
          {control}
        </div>
      </div>
      {children}
    </div>
  );
}

/** A menu sized for a row: its list opens wider than the trigger, to the left. */
function Picker<V extends string>(props: {
  value: V;
  options: DropdownOption<V>[];
  onChange: (value: V) => void;
  disabled?: boolean;
}) {
  return <Dropdown {...props} menuWidth={184} align="end" />;
}

function SwitchRow({
  title,
  description,
  checked,
  disabled = false,
  onChange,
}: {
  title: string;
  description?: string;
  checked: boolean;
  disabled?: boolean;
  onChange: (checked: boolean) => void;
}) {
  // By touch the whole row is the switch, as it is everywhere else on a phone.
  if (IS_MOBILE) {
    return (
      <ToggleRow
        title={title}
        description={description}
        checked={checked}
        disabled={disabled}
        onChange={onChange}
      />
    );
  }

  return (
    <div className="flex min-h-[52px] items-center gap-3 py-2 pl-4 pr-3">
      <div className="min-w-0 flex-1">
        <p className="text-[13.5px] text-fg">{title}</p>
        {description && (
          <p className="mt-0.5 text-[12.5px] leading-snug text-fg-muted">{description}</p>
        )}
      </div>
      <Toggle checked={checked} onChange={onChange} disabled={disabled} label={title} />
    </div>
  );
}

/** A group's footnote: the one thing worth saying about it, under it. */
function Footnote({ children }: { children: ReactNode }) {
  return <p className={cn('mt-1.5 px-4 leading-snug text-fg-muted', NOTE_TEXT)}>{children}</p>;
}

// -- the custom rate ------------------------------------------------------------

/**
 * The video's own rate, typed in Mbps.
 *
 * The field keeps its own text so a half-typed number ("2.", "0") is left for
 * the user to finish instead of being corrected under their cursor. Every
 * keystroke that makes a usable rate is committed there and then -- the size
 * underneath answers as they type -- and one that does not waits: it is only
 * called out when the user leaves the field with it, and never replaced.
 */
function BitrateField({
  kbps,
  audioKbps,
  keptSec,
  onCommit,
}: {
  kbps: number;
  audioKbps: number;
  keptSec: number;
  onCommit: (kbps: number) => void;
}) {
  const { t, language } = useTranslation();
  const [draft, setDraft] = useState(() => formatMbps(kbps, language));
  const [invalid, setInvalid] = useState(false);

  // An undo moves the rate without the field. The text follows it, unless it
  // already says the same number in other words ("2.50" is 2.5 and stays).
  const [seen, setSeen] = useState(kbps);
  if (seen !== kbps) {
    setSeen(kbps);
    if (parseMbps(draft) !== kbps) {
      setDraft(formatMbps(kbps, language));
      setInvalid(false);
    }
  }

  const change = (text: string) => {
    setDraft(text);
    const next = parseMbps(text);
    if (next == null) return;
    setInvalid(false);
    if (next !== kbps) onCommit(next);
  };

  const settle = () => {
    const next = parseMbps(draft);
    if (next == null) {
      setInvalid(true);
      return;
    }
    setInvalid(false);
    // Tidied once the user is done with it: "2." becomes "2".
    setDraft(formatMbps(next, language));
    if (next !== kbps) onCommit(next);
  };

  return (
    <TextInput
      value={draft}
      inputMode="decimal"
      autoComplete="off"
      spellCheck={false}
      aria-label={t('editor.qualityCustom')}
      onChange={(event) => change(event.target.value)}
      onBlur={settle}
      onKeyDown={(event) => {
        if (event.key === 'Enter') settle();
        if (event.key === 'Escape') {
          setDraft(formatMbps(kbps, language));
          setInvalid(false);
        }
      }}
      trailing={<span className="text-[12.5px] text-fg-muted">Mbps</span>}
      error={invalid ? t('editor.bitrateInvalid') : null}
      hint={t('editor.estimatedSize', {
        size: formatBytes(estimatedBytes(kbps + audioKbps, keptSec)),
      })}
    />
  );
}

import { AnimatePresence, motion, useIsPresent, type Variants } from 'motion/react';
import { open } from '@tauri-apps/plugin-dialog';
import { getCurrentWebview } from '@tauri-apps/api/webview';
import {
  CircleAlert,
  FilePlus2,
  FolderOpen,
  Link2,
  Pause,
  Play,
  Redo2,
  Scissors,
  Trash2,
  Undo2,
  X,
} from 'lucide-react';
import {
  useCallback,
  useEffect,
  useEffectEvent,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
  type MutableRefObject,
  type ReactNode,
  type RefObject,
} from 'react';

import { LinkImportModal } from '@/components/editor/LinkImportModal';
import { MediaRail } from '@/components/editor/MediaRail';
import { OutputInspector } from '@/components/editor/OutputInspector';
import { PlayheadTime, createPlayhead } from '@/components/editor/playhead';
import { PreviewStage } from '@/components/editor/PreviewStage';
import { RecentDownloads } from '@/components/editor/RecentDownloads';
import { TimelineDock } from '@/components/editor/TimelineDock';
import { Button } from '@/components/ui/Button';
import { IconButton } from '@/components/ui/IconButton';
import { InlineNotice } from '@/components/ui/InlineNotice';
import { Modal } from '@/components/ui/Modal';
import { PageHeader } from '@/components/ui/PageHeader';
import { Progress } from '@/components/ui/Progress';
import { errorMessage, useTranslation } from '@/i18n';
import { cn } from '@/lib/cn';
import { VIDEO_EXTENSIONS } from '@/lib/editor/files';
import { cutAt, nextKeptPosition } from '@/lib/editor/segments';
import { ZOOM_STEP } from '@/lib/editor/zoom';
import { COLLAPSE, EDITOR, FADE, T } from '@/lib/motion';
import { formatMbps } from '@/lib/editor/bitrate';
import {
  basename,
  formatBytes,
  formatFrameTimecode,
  formatTimecode,
  prettyCodec,
} from '@/lib/format';
import { IS_MOBILE, openFile, revealFile } from '@/lib/platform';
import * as ipc from '@/services/ipc';
import {
  selectActiveClip,
  selectAnyHistory,
  selectCuts,
  selectHistory,
  selectKeptLength,
  selectOptions,
  useEditorStore,
} from '@/stores/useEditorStore';
import { useToolsStore } from '@/stores/useToolsStore';
import type { Settings } from '@/types';

/** How many buckets and cells the timeline asks for. */
const WAVEFORM_BUCKETS = 4000;
const FILMSTRIP_CELLS = 40;
/**
 * The height the frames are rendered at, in device pixels. A phone's track is
 * shorter than the desktop's but its screen is two or three pixels to the
 * point, and frames rendered for the desktop were drawn there at a third of
 * their resolution; twice the track is as sharp as a thumbnail needs to be.
 */
const FILMSTRIP_CELL_HEIGHT = IS_MOBILE ? 112 : 68;

const DROP_TRANSITION = 'transition-[background-color,box-shadow] duration-150 ease-out-quint';

/**
 * The phone's empty screen. It leaves lifted out of the flow (the `popLayout`
 * it is shown under), placed by its offset in the page -- which scrolling does
 * not change, so scrolled down to the recent downloads it dropped by as much
 * as it fades. It is moved back up by that much at once, and fades where it
 * was seen.
 */
const EMPTY_SCREEN: Variants = {
  ...FADE,
  exit: (scrolled: number) => ({
    opacity: 0,
    y: -scrolled,
    transition: { ...T.componentOut, y: { duration: 0 } },
  }),
};

/** How far the page under an element is scrolled, as its offset leaves out. */
function scrolledUnder(element: HTMLElement | null): number {
  const parent = element?.offsetParent;
  if (!element || !(parent instanceof HTMLElement)) return 0;
  return parent.getBoundingClientRect().top + element.offsetTop - element.getBoundingClientRect().top;
}

/** Whatever is drawn over the editor and has the keyboard while it is open. */
const OVERLAY = '[role="dialog"][aria-modal="true"],[role="combobox"][aria-expanded="true"]';

/** Inputs that hold no text, so there is nothing of theirs for Ctrl+Z to take back. */
const TEXTLESS_INPUTS = new Set([
  'button',
  'checkbox',
  'color',
  'file',
  'hidden',
  'image',
  'radio',
  'range',
  'reset',
  'submit',
]);

/** A field whose text is being edited, where the text's own undo has to win. */
function isTextField(element: Element | null): boolean {
  if (!(element instanceof HTMLElement)) return false;
  if (element.isContentEditable || element instanceof HTMLTextAreaElement) return true;
  return element instanceof HTMLInputElement && !TEXTLESS_INPUTS.has(element.type);
}

/** A control that answers the plain keys itself: a field, a slider, a menu. */
function hasOwnKeys(element: Element | null): boolean {
  if (!(element instanceof HTMLElement)) return false;
  return (
    element.isContentEditable ||
    element instanceof HTMLInputElement ||
    element instanceof HTMLTextAreaElement ||
    element instanceof HTMLSelectElement ||
    element.closest('[role="combobox"],[role="listbox"],[role="slider"]') != null
  );
}

interface EditorPageProps {
  settings: Settings;
  /**
   * Filled in, on a phone, with what the system Back gesture does while a clip
   * is open and nothing is open over it: the shell hears the gesture, and the
   * editor decides what it means -- close, asking first if there is work to
   * lose.
   */
  onBackRef?: MutableRefObject<(() => void) | null>;
}

/**
 * The editor.
 *
 * Two shapes, one route. With nothing open it is an ordinary page in the app's
 * centred column -- a place to drop a file or paste a link. With something open
 * it takes the whole area and becomes a workspace: a rail of clips, the
 * picture, the settings, and the file laid out in time underneath. The screen
 * only ambushes the user once they have asked it to.
 *
 * On a phone the same two shapes are a screen and a mode. Nothing open, it is
 * a card under the app's own bars. A clip open, the bars step aside and the
 * editor has the whole screen, laid out in a column for a thumb: its own bar
 * with the way out and the export, the picture, the transport, the timeline,
 * and the settings filling what is left.
 */
export function EditorPage({ settings, onBackRef }: EditorPageProps) {
  const { t, language } = useTranslation();

  const pool = useEditorStore((state) => state.pool);
  const clip = useEditorStore(selectActiveClip);
  const cuts = useEditorStore(selectCuts);
  const options = useEditorStore(selectOptions);
  const keptLength = useEditorStore(selectKeptLength);
  const opening = useEditorStore((state) => state.opening);
  const openError = useEditorStore((state) => state.openError);
  const timeline = useEditorStore((state) => state.timeline);
  const job = useEditorStore((state) => state.job);
  const submitting = useEditorStore((state) => state.submitting);
  const submitError = useEditorStore((state) => state.submitError);
  const canUndo = useEditorStore((state) => selectHistory(state).past.length > 0);
  const canRedo = useEditorStore((state) => selectHistory(state).future.length > 0);
  const anyHistory = useEditorStore(selectAnyHistory);

  const openPath = useEditorStore((state) => state.open);
  const activate = useEditorStore((state) => state.activate);
  const closeClip = useEditorStore((state) => state.closeClip);
  const closeAll = useEditorStore((state) => state.closeAll);
  const split = useEditorStore((state) => state.split);
  const removeCut = useEditorStore((state) => state.remove);
  const dragEdge = useEditorStore((state) => state.drag);
  const beginGesture = useEditorStore((state) => state.beginGesture);
  const endGesture = useEditorStore((state) => state.endGesture);
  const undo = useEditorStore((state) => state.undo);
  const redo = useEditorStore((state) => state.redo);
  const loadKeyframes = useEditorStore((state) => state.loadKeyframes);
  const startExport = useEditorStore((state) => state.startExport);
  const cancelExport = useEditorStore((state) => state.cancelExport);

  const ffmpeg = useToolsStore((state) => state.tools?.ffmpeg ?? null);
  const installing = useToolsStore((state) => state.installing.ffmpeg);
  const installTool = useToolsStore((state) => state.install);
  const ffmpegReady = ffmpeg?.available ?? false;

  // No Android build of FFmpeg this app ships has a hardware encoder, so a
  // phone's clips start without one whatever the setting says.
  const hardware = !IS_MOBILE && settings.hardwareAcceleration;

  const videoRef = useRef<HTMLVideoElement>(null);
  const surfaceRef = useRef<HTMLDivElement>(null);
  // The dock fills this in with its own zoom, which is the only way the keys
  // the toolbar advertises can reach a scale that lives inside it.
  const zoomRef = useRef<{ by: (factor: number) => void; fit: () => void } | null>(null);
  // Where the playhead is. Not state: it moves on every frame of playback, and
  // the few things that follow it subscribe (see playhead.tsx).
  const [playhead] = useState(createPlayhead);
  const [playing, setPlaying] = useState(false);
  const [playable, setPlayable] = useState(true);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [snapping, setSnapping] = useState(true);
  const [dragging, setDragging] = useState(false);
  const [picking, setPicking] = useState(false);
  const [pickFailed, setPickFailed] = useState(false);
  const [installError, setInstallError] = useState<string | null>(null);
  const [linkOpen, setLinkOpen] = useState(false);
  // A recent download being opened says so on its own row, so the card's
  // button above it stands still instead of spinning as well.
  const [openingRecent, setOpeningRecent] = useState(false);
  // Read as an open begins, while the phone's empty screen is still where it
  // is seen; see EMPTY_SCREEN.
  const emptyRef = useRef<HTMLDivElement>(null);
  const [emptyScrolled, setEmptyScrolled] = useState(0);
  const [discardOpen, setDiscardOpen] = useState(false);
  const [resultError, setResultError] = useState<string | null>(null);
  // Whether the preview has to stop at 100 %, which the inspector then says.
  const [previewCapped, setPreviewCapped] = useState(false);

  // Validated against the list rather than trusted: a split replaces the piece
  // it divides, so an id held from before it would name something that is no
  // longer there -- and the Delete button would offer to remove it.
  const selected = cuts.find((cut) => cut.id === selectedId) ?? null;

  const duration = clip?.probe.durationSec ?? 0;
  const fps = clip?.probe.fps ?? null;
  const frameStep = fps && fps > 0 ? 1 / fps : 0.04;
  const running = job.status === 'running';
  const losslessMode = options?.mode === 'lossless';

  // -- opening -------------------------------------------------------------

  const pickFile = useCallback(async () => {
    setPickFailed(false);
    setPicking(true);
    try {
      let paths: string[];
      if (IS_MOBILE) {
        // The Android picker hands back content addresses FFmpeg cannot open;
        // the platform side copies what was picked into the app's cache and
        // returns those paths. It offers sound as well as pictures, and the
        // store is what turns a file without a picture away.
        paths = await ipc.platformPickMediaFiles();
      } else {
        const selected = await open({
          multiple: true,
          filters: [{ name: t('editor.videoFiles'), extensions: VIDEO_EXTENSIONS }],
        });
        paths = Array.isArray(selected) ? selected : selected ? [selected] : [];
      }
      for (const path of paths) await openPath(path, hardware);
    } catch {
      // Only the picker can land here -- opening reports its own failures. What
      // it rejects with is a bare sentence in English, from the dialog or from
      // a phone that could not copy what was chosen, so the editor says it in
      // its own words instead.
      setPickFailed(true);
    } finally {
      setPicking(false);
    }
  }, [hardware, openPath, t]);

  // Stable, for the memoised children they are handed to.
  const addClip = useCallback(() => void pickFile(), [pickFile]);
  const openLink = useCallback(() => setLinkOpen(true), []);
  const closeLink = useCallback(() => setLinkOpen(false), []);
  // A clip from a link is a new attempt like a pick or a drop, so it clears a
  // picker's failure as they do: left standing, the sentence stayed under the
  // clip it had nothing to do with, and came back when that clip was closed.
  const openFetched = useCallback(
    (path: string) => {
      setPickFailed(false);
      void openPath(path, hardware);
    },
    [hardware, openPath],
  );
  // A recent download is one more way to pick, and clears a picker's failure
  // the same way.
  const openRecent = useCallback(
    async (path: string) => {
      setPickFailed(false);
      setOpeningRecent(true);
      try {
        return await openPath(path, hardware);
      } finally {
        setOpeningRecent(false);
      }
    },
    [hardware, openPath],
  );

  // A file dropped on the window arrives as an OS event, and only while this
  // screen is up: dropping one on Home has nothing to do there. A phone has no
  // window to drop anything on.
  useEffect(() => {
    if (IS_MOBILE) return;
    const pending = getCurrentWebview().onDragDropEvent((event) => {
      if (event.payload.type === 'over') {
        setDragging(true);
        return;
      }
      setDragging(false);
      if (event.payload.type === 'drop') {
        setPickFailed(false);
        for (const path of event.payload.paths) {
          void openPath(path, hardware);
        }
      }
    });
    return () => {
      void pending.then((unlisten) => unlisten());
    };
  }, [hardware, openPath]);

  // A new clip is a new element: the playhead, and whether the last file could
  // be decoded at all, say nothing about this one.
  //
  // Keyed on the clip's identity rather than on the object, because the object
  // is replaced when its keyframes land -- a minute into the edit on a large
  // file -- and re-running this then would quietly throw the playhead back to
  // the start and deselect the piece the user had just split off.
  //
  // The focus is here too: the transport's keys only answer when the keyboard
  // is aimed at the editor, and a clip opened from the sidebar or the Home
  // screen would otherwise leave it aimed somewhere else.
  useLayoutEffect(() => {
    if (IS_MOBILE && opening && !clip) setEmptyScrolled(scrolledUnder(emptyRef.current));
  }, [opening, clip]);

  const clipId = clip?.id ?? null;
  useEffect(() => {
    playhead.set(0);
    setPlaying(false);
    setPlayable(true);
    setSelectedId(null);
    if (clipId) {
      surfaceRef.current?.focus();
      void loadKeyframes();
    }
  }, [clipId, loadKeyframes, playhead]);

  // -- the playhead --------------------------------------------------------

  const seek = useCallback(
    (seconds: number) => {
      const bounded = Math.max(0, seconds);
      playhead.set(bounded);
      const video = videoRef.current;
      // A file that never decoded still has a timeline; only the picture is
      // missing, so a failed seek must not take the marks down with it.
      if (video && Number.isFinite(bounded)) {
        try {
          video.currentTime = bounded;
        } catch {
          // Not seekable yet. The playhead has already moved, which is what the
          // marks are read against.
        }
      }
    },
    [playhead],
  );

  // What is being watched is the film being made, not the one it came from, so
  // playback steps over the pieces that have been thrown away. Asked on every
  // position the video reports while it plays, which also catches the piece
  // under the playhead being deleted mid-play.
  const stepOver = useEffectEvent((video: HTMLVideoElement) => {
    if (video.paused || cuts.length === 0) return;
    const seconds = video.currentTime;
    if (cutAt(cuts, seconds)) return;
    const next = nextKeptPosition(cuts, seconds);
    if (next == null) {
      video.pause();
      return;
    }
    seek(next);
  });

  // `timeupdate` fires about four times a second, which is a playhead that
  // hops. While the video actually plays the position is read per frame
  // instead, and the loop stops the moment it pauses. The event is kept as
  // well, because the frame loop never runs while the window is hidden and
  // nothing in the page can detect that state.
  //
  // Every one of these goes to the playhead and to nothing else: a frame of
  // playback renders nothing in React (see playhead.tsx).
  useEffect(() => {
    const video = videoRef.current;
    if (!video) return;
    let frame = 0;

    const report = () => {
      playhead.set(video.currentTime);
      stepOver(video);
    };
    const follow = () => {
      report();
      frame = requestAnimationFrame(follow);
    };
    const start = () => {
      setPlaying(true);
      cancelAnimationFrame(frame);
      frame = requestAnimationFrame(follow);
    };
    const stop = () => {
      setPlaying(false);
      cancelAnimationFrame(frame);
      playhead.set(video.currentTime);
    };

    video.addEventListener('play', start);
    video.addEventListener('pause', stop);
    video.addEventListener('ended', stop);
    video.addEventListener('seeked', report);
    video.addEventListener('timeupdate', report);
    return () => {
      cancelAnimationFrame(frame);
      video.removeEventListener('play', start);
      video.removeEventListener('pause', stop);
      video.removeEventListener('ended', stop);
      video.removeEventListener('seeked', report);
      video.removeEventListener('timeupdate', report);
    };
  }, [clip?.id, playhead]);

  const playPause = useCallback(() => {
    const video = videoRef.current;
    if (!video) return;
    if (video.paused) {
      void video.play().catch((error: unknown) => {
        // A pause that arrives before playback has started -- a second tap,
        // or the end of the kept film reached at once -- rejects the play it
        // interrupted. Right after a seek that window is as long as the
        // decoder takes, and reading it as a file that cannot play took the
        // picture away for the rest of the edit.
        if (error instanceof DOMException && error.name === 'AbortError') return;
        setPlayable(false);
      });
    } else {
      video.pause();
    }
  }, []);

  const step = useCallback(
    (frames: number) => {
      videoRef.current?.pause();
      seek(Math.min(Math.max(playhead.get() + frames * frameStep, 0), duration));
    },
    [duration, frameStep, playhead, seek],
  );

  const markUnplayable = useCallback(() => setPlayable(false), []);
  const toggleSnapping = useCallback(() => setSnapping((value) => !value), []);

  // The position under the desktop's picture, to the frame. Kept stable so the
  // readout's subscription is not made again on every render; the phone's
  // transport reads in tenths instead, which is what a thumb can place.
  const framePosition = useCallback((seconds: number) => formatFrameTimecode(seconds, fps), [fps]);

  // -- what the timeline is drawn from -------------------------------------

  const tokenRef = useRef(0);
  const lastRequestRef = useRef<string | null>(null);
  const requestTimeline = useCallback(
    (startSec: number, lengthSec: number) => {
      if (!clip || !ffmpegReady) return;
      // The whole file, unless the window has been zoomed past what a
      // whole-file pass can resolve: 4000 buckets over an hour is a bucket
      // every 0.9 s, which is plenty across a whole track and useless inside a
      // twenty-second window.
      const windowed = lengthSec > 0 && lengthSec < duration / 8;
      // Panning across a whole-file view asks for the same bytes it is already
      // showing, and asking again is not free: a new token supersedes the work
      // in flight, which blanks both tracks before three ffprobes run to draw
      // exactly what was there. Three decimals is the precision the cache on
      // the other side keys on, so two windows that match here really do agree.
      const key = windowed
        ? `${clip.id}|${startSec.toFixed(3)}|${lengthSec.toFixed(3)}`
        : `${clip.id}|all`;
      if (key === lastRequestRef.current) return;
      lastRequestRef.current = key;
      tokenRef.current += 1;
      const token = tokenRef.current;
      // A window that was never really asked for must not count as the one on
      // screen, or the next gesture back to it would be skipped as a repeat.
      const forget = () => {
        if (lastRequestRef.current === key) lastRequestRef.current = null;
      };
      const base = {
        path: clip.path,
        startSec: windowed ? startSec : null,
        lengthSec: windowed ? lengthSec : null,
        token,
      };
      void ipc
        .requestTimeline({
          ...base,
          kind: 'waveform',
          count: WAVEFORM_BUCKETS,
          cellHeight: null,
        })
        .catch(forget);
      void ipc
        .requestTimeline({
          ...base,
          kind: 'filmstrip',
          count: FILMSTRIP_CELLS,
          cellHeight: FILMSTRIP_CELL_HEIGHT,
        })
        .catch(forget);
    },
    [clip, duration, ffmpegReady],
  );

  // A split leaves the playhead between two new pieces. The one it opens is
  // the one the user is about to work on, so that is the one that is selected;
  // leaving nothing selected would mean reaching for the piece by hand before
  // Delete could mean anything.
  const splitHere = useCallback(() => {
    const at = playhead.get();
    split(at);
    const next = selectCuts(useEditorStore.getState());
    setSelectedId((next.find((cut) => cut.startSec === at) ?? cutAt(next, at))?.id ?? null);
  }, [playhead, split]);

  const deleteSelected = useCallback(() => {
    if (!selected) return;
    removeCut(selected.id);
    setSelectedId(null);
  }, [removeCut, selected]);

  // -- the keyboard --------------------------------------------------------
  //
  // Heard on the window, in the capture phase, for as long as a clip is open
  // and this screen is the one showing. The window, because focus does not stay
  // on the editor: whatever held it -- a dialog, a menu, a piece just deleted
  // -- hands it to the body when it goes, and a key map bound to the surface
  // went deaf until the user clicked back in. The capture phase, so the editor
  // hears a key before the app's own accelerators do and can keep two of them
  // from pulling the user off this screen mid-edit: Ctrl+V and Ctrl+Enter both
  // navigate Home.
  const onKeyDown = useEffectEvent((event: KeyboardEvent) => {
    if (!clip || event.defaultPrevented || event.isComposing) return;

    const target = event.target instanceof Element ? event.target : null;
    const take = () => {
      event.preventDefault();
      event.stopPropagation();
    };
    const key = event.key;
    const control = event.ctrlKey || event.metaKey;

    // Held back wherever it was pressed, a field or the link sheet included:
    // the app's accelerator for a download would take this whole screen away.
    if (control && key === 'Enter') {
      take();
      return;
    }

    // A dialog, or a menu that is open, has the keyboard to itself -- all but
    // Ctrl+V, which outside a field is the app's accelerator for Home and would
    // take the screen away from under the dialog.
    if (document.querySelector(OVERLAY)) {
      if (control && key.toLowerCase() === 'v' && !isTextField(target)) event.stopPropagation();
      return;
    }

    if (control) {
      // The rest are the screen's from anywhere on it -- a menu's button, the
      // slider just let go of, the body -- except a text field, where Ctrl+Z
      // and Ctrl+V belong to the text being typed.
      if (isTextField(target)) return;
      switch (key.toLowerCase()) {
        case 'z':
          take();
          if (event.shiftKey) redo();
          else undo();
          break;
        case 'y':
          take();
          redo();
          break;
        case 'b':
          take();
          splitHere();
          break;
        case 'v':
          take();
          setLinkOpen(true);
          break;
        case 'i':
          take();
          void pickFile();
          break;
        case 'e':
          take();
          if (!running) void startExport();
          break;
        case '0':
          take();
          zoomRef.current?.fit();
          break;
        default:
          break;
      }
      return;
    }

    // The transport's keys answer only when the keyboard is aimed at the
    // editor -- or at nothing in particular -- and never at a control with keys
    // of its own: Space opens a menu, the arrows move a slider, and Delete would
    // take the selected piece away from behind a field.
    if (target && target !== document.body && !surfaceRef.current?.contains(target)) return;
    if (hasOwnKeys(target)) return;

    switch (key) {
      case ' ':
        take();
        playPause();
        break;
      case 'ArrowLeft':
        take();
        step(event.shiftKey ? -Math.round(1 / frameStep) : -1);
        break;
      case 'ArrowRight':
        take();
        step(event.shiftKey ? Math.round(1 / frameStep) : 1);
        break;
      case 'Home':
        take();
        seek(0);
        break;
      case 'End':
        take();
        seek(duration);
        break;
      case 'i':
      case 'I':
        take();
        if (selected) dragEdge(selected.id, 'start', playhead.get());
        break;
      case 'o':
      case 'O':
        take();
        if (selected) dragEdge(selected.id, 'end', playhead.get());
        break;
      case 'Delete':
      case 'Backspace':
        take();
        deleteSelected();
        break;
      case 's':
      case 'S':
        take();
        setSnapping((value) => !value);
        break;
      case 'm':
      case 'M':
        take();
        if (videoRef.current) videoRef.current.muted = !videoRef.current.muted;
        break;
      case '+':
      case '=':
        take();
        zoomRef.current?.by(ZOOM_STEP);
        break;
      case '-':
      case '_':
        take();
        zoomRef.current?.by(1 / ZOOM_STEP);
        break;
      case 'Escape':
        take();
        setSelectedId(null);
        break;
      default:
        break;
    }
  });

  // Only while this screen is the one showing: on the way out it is still
  // mounted for as long as its exit takes, and the screen coming in owns the
  // keyboard from the moment it was asked for.
  const present = useIsPresent();
  const listening = clipId != null && present;
  useEffect(() => {
    if (!listening) return;
    window.addEventListener('keydown', onKeyDown, true);
    return () => window.removeEventListener('keydown', onKeyDown, true);
  }, [listening]);

  // Closing takes every clip and every cut with it. Worth a question only when
  // there is something to lose: a clip opened and not yet touched is no more
  // than a file the user can open again in one press, and asking about it would
  // be the kind of dialog people learn to dismiss without reading. Any clip:
  // the one on screen is not the only one that goes.
  const hasEdits = cuts.length > 1 || anyHistory || cuts[0]?.startSec !== 0;

  const requestClose = useCallback(() => {
    if (hasEdits) setDiscardOpen(true);
    else closeAll();
  }, [closeAll, hasEdits]);

  // The phone's Back gesture, while a clip is open and nothing stands over
  // it: the X. An open menu, the link sheet and the question about discarding
  // are layers of their own, and the shell takes those down first (see
  // useBackLayer). A question this opens can be answered either way and asked
  // again: the shell keeps the editor's place in the history while it stays.
  useEffect(() => {
    if (!IS_MOBILE || !onBackRef || !listening) return;
    onBackRef.current = requestClose;
    return () => {
      if (onBackRef.current === requestClose) onBackRef.current = null;
    };
  }, [listening, onBackRef, requestClose]);

  // A phone's keyboard, up for one of the editor's own fields, which the
  // picture and the timeline fold away for; and a phone on its side, where the
  // editor scrolls as a whole (see the layout below).
  const { typing, sideways } = usePhoneRoom(surfaceRef, IS_MOBILE && clipId != null);

  // The field being typed into, and the line under it that answers it, above
  // the keyboard once there is room for them. The browser brings the field
  // itself into view as the keyboard rises, but not what is written beneath
  // it -- the custom rate's size estimate was left under the keyboard.
  const revealField = useCallback(() => {
    const field = document.activeElement;
    if (!(field instanceof HTMLElement) || !surfaceRef.current?.contains(field)) return;
    (field.closest<HTMLElement>('[data-keyboard-anchor]') ?? field).scrollIntoView({
      block: 'nearest',
    });
  }, []);

  // The keyboard can change height while it is up -- another layout, the
  // suggestions strip -- and the field goes on needing to be seen.
  useEffect(() => {
    if (!typing) return;
    const viewport = window.visualViewport ?? window;
    let frame = 0;
    const onResize = () => {
      cancelAnimationFrame(frame);
      frame = requestAnimationFrame(revealField);
    };
    viewport.addEventListener('resize', onResize);
    return () => {
      cancelAnimationFrame(frame);
      viewport.removeEventListener('resize', onResize);
    };
  }, [revealField, typing]);

  const installFfmpeg = useCallback(async () => {
    setInstallError(null);
    if (await installTool('ffmpeg')) return;
    setInstallError(useToolsStore.getState().error ?? t('error.network.message'));
  }, [installTool, t]);

  // A finished file is opened by whatever the system hands it to, and on a
  // phone there may be nothing that will take it. That is said beside the
  // button, and forgotten when the next export replaces the file.
  const openResult = useCallback(async (path: string) => {
    setResultError(null);
    try {
      await openFile(path);
    } catch (caught) {
      setResultError(ipc.toAppError(caught).message);
    }
  }, []);
  useEffect(() => {
    setResultError(null);
  }, [job.outputPath]);

  const railClips = useMemo(
    () =>
      pool.map((entry) => ({
        id: entry.id,
        name: entry.name,
        path: entry.path,
        durationSec: entry.probe.durationSec ?? 0,
      })),
    [pool],
  );

  // `language` is in the list for the words in it: `t` itself never changes,
  // so without it a language switch left "Lossless" standing in Turkish.
  const summary = useMemo(() => {
    if (!options || !clip) return '';
    const parts: string[] = [];
    const height = options.maxHeight ?? clip.probe.height;
    if (height) parts.push(`${height}p`);
    if (options.mode === 'lossless') {
      parts.push(t('editor.lossless'));
    } else {
      parts.push(prettyCodec(options.videoCodec) ?? options.videoCodec);
      // A rate the user typed is the one number they will want to see again.
      if (options.videoBitrateKbps != null) {
        parts.push(`${formatMbps(options.videoBitrateKbps, language)} Mbps`);
      }
    }
    if (options.fps) parts.push(`${options.fps} fps`);
    parts.push(options.container.toUpperCase());
    return parts.join(' · ');
  }, [clip, language, options, t]);

  const shownOpenError = pickFailed
    ? t('editor.openFailed')
    : openError
      ? errorMessage(openError)
      : null;

  // The phone's clips, at the top of the Clip tab. Built here, where the ways
  // of adding one live, and memoised so the inspector it is handed to can skip
  // the renders that do not concern it.
  const clipStrip = useMemo(
    () =>
      IS_MOBILE && clipId ? (
        <>
          <MediaRail
            orientation="horizontal"
            clips={railClips}
            activeId={clipId}
            onActivate={activate}
            onRemove={closeClip}
            onAdd={addClip}
            onAddLink={openLink}
            adding={picking || opening}
            className="-mx-4 px-4"
          />
          {shownOpenError && (
            <InlineNotice tone="error" className="mt-2">
              {shownOpenError}
            </InlineNotice>
          )}
        </>
      ) : undefined,
    [activate, addClip, clipId, closeClip, openLink, opening, picking, railClips, shownOpenError],
  );

  const ffmpegNotice = ffmpeg != null && !ffmpegReady && (
    <div className="flex items-center gap-3 rounded-[var(--radius-card)] border border-card-edge bg-surface px-4 py-3">
      <CircleAlert size={16} aria-hidden="true" className="shrink-0 text-warning" />
      <div className="min-w-0 flex-1">
        <p className="text-[13.5px] font-medium text-fg">{t('convert.ffmpegRequired')}</p>
        <p className="mt-0.5 text-[12.5px] leading-relaxed text-fg-muted">
          {t('editor.ffmpegRequiredBody')}
        </p>
        {installError && (
          <InlineNotice tone="error" className="mt-1.5">
            {installError}
          </InlineNotice>
        )}
      </div>
      <Button
        size="sm"
        variant="secondary"
        loading={installing != null}
        onClick={() => void installFfmpeg()}
      >
        {installing != null ? t('settings.toolInstalling') : t('settings.toolInstall')}
      </Button>
    </div>
  );

  const linkSheet = (
    <LinkImportModal open={linkOpen} onClose={closeLink} onFetched={openFetched} />
  );

  const discardQuestion = (
    <Modal
      open={discardOpen}
      onClose={() => setDiscardOpen(false)}
      title={t('editor.discardTitle')}
      description={t('editor.discardBody')}
      closeLabel={t('common.close')}
      width={400}
      footer={
        <div className="flex justify-end gap-2">
          <Button size="sm" variant="secondary" onClick={() => setDiscardOpen(false)}>
            {t('common.cancel')}
          </Button>
          <Button
            size="sm"
            variant="danger"
            onClick={() => {
              setDiscardOpen(false);
              closeAll();
            }}
          >
            {t('editor.discardConfirm')}
          </Button>
        </div>
      }
    />
  );

  // -- the phone -------------------------------------------------------------

  if (IS_MOBILE) {
    const failed = job.status === 'failed' && job.error;
    const done = job.status === 'completed' && job.outputPath;
    // The picture's box is the clip's own shape across the width of the
    // screen, so a landscape video fills it with no bars at all -- and never
    // more than about a third of the height, so a portrait one leaves room for
    // everything under it.
    const width = clip?.probe.width ?? 0;
    const height = clip?.probe.height ?? 0;
    const shape = width > 0 && height > 0 ? height / width : 9 / 16;

    return (
      <>
        {/* The two shapes pass each other: the one leaving is lifted out of the
            flow while it fades, so the one arriving is laid out where it will
            stay from its first frame. The dialogs stand outside both, or one
            taken down in the same moment would be frozen open in the copy of
            the shape that is leaving. */}
        <AnimatePresence initial={false} mode="popLayout" custom={emptyScrolled}>
          {clip ? (
            <motion.div
              key="editing"
              ref={surfaceRef}
              variants={EDITOR}
              initial="initial"
              animate="animate"
              exit="exit"
              tabIndex={-1}
              className="flex min-h-0 flex-1 flex-col outline-none"
            >
              {/* -- the bar ----------------------------------------------- */}
              <div className="flex h-14 shrink-0 items-center gap-1 pl-1.5 pr-3">
                <TouchButton label={t('editor.close')} onClick={requestClose}>
                  <X size={22} />
                </TouchButton>
                <div className="min-w-0 flex-1 pl-0.5">
                  <p className="truncate text-[15px] font-semibold leading-5 text-fg">{clip.name}</p>
                  <p className="tabular truncate text-[12.5px] leading-4 text-fg-muted">
                    {t('editor.length')} {formatTimecode(keptLength)}
                  </p>
                </div>
                <IconButton
                  icon={<Undo2 size={20} />}
                  label={t('editor.undo')}
                  disabled={!canUndo}
                  onClick={undo}
                />
                <IconButton
                  icon={<Redo2 size={20} />}
                  label={t('editor.redo')}
                  disabled={!canRedo}
                  onClick={redo}
                />
                {running ? (
                  <Button
                    size="sm"
                    variant="secondary"
                    className="ml-1"
                    onClick={() => void cancelExport()}
                  >
                    {t('common.cancel')}
                  </Button>
                ) : (
                  <Button
                    size="sm"
                    variant="primary"
                    className="ml-1"
                    loading={submitting}
                    disabled={!ffmpegReady}
                    onClick={() => void startExport()}
                  >
                    {t('editor.export')}
                  </Button>
                )}
              </div>

              {/* -- what the export said ---------------------------------- */}
              {/* Under the button that asked for it, and only while there is
                  something to say. */}
              <AnimatePresence initial={false}>
                {(running || submitError || failed || done) && (
                  <motion.div
                    variants={COLLAPSE}
                    initial="initial"
                    animate="animate"
                    exit="exit"
                    className="shrink-0 overflow-hidden"
                  >
                    <div className="px-4 pb-3 pt-0.5">
                      {running && (
                        <>
                          <div className="flex items-baseline justify-between gap-3 text-[13px] text-fg-muted">
                            <span>{t('editor.exporting')}</span>
                            {job.percent != null && (
                              <span className="tabular">
                                {t('editor.percent', { value: Math.round(job.percent) })}
                              </span>
                            )}
                          </div>
                          <Progress value={job.percent} className="mt-2" label={t('editor.exporting')} />
                        </>
                      )}
                      {submitError && (
                        <InlineNotice tone="error">{errorMessage(submitError)}</InlineNotice>
                      )}
                      {failed && (
                        <InlineNotice tone="error">
                          {job.error!.code === 'unknown'
                            ? t('editor.exportFailed')
                            : errorMessage(job.error!)}
                        </InlineNotice>
                      )}
                      {done && (
                        <>
                          <div className="flex items-center gap-3">
                            <div className="min-w-0 flex-1">
                              <p className="text-[14px] font-medium text-fg">{t('editor.exported')}</p>
                              {/* The file's own name: the folder is the phone's
                                  downloads, and its path is not something
                                  anyone reads. */}
                              <p className="mt-0.5 truncate text-[12.5px] text-fg-muted">
                                {basename(job.outputPath!)}
                              </p>
                            </div>
                            <Button
                              size="sm"
                              variant="secondary"
                              onClick={() => void openResult(job.outputPath!)}
                            >
                              {t('common.open')}
                            </Button>
                          </div>
                          {resultError && (
                            <InlineNotice tone="error" className="mt-2">
                              {resultError}
                            </InlineNotice>
                          )}
                        </>
                      )}
                    </div>
                  </motion.div>
                )}
              </AnimatePresence>

              {/* -- the picture, the transport, the file in time ---------- */}
              {/* All three step aside while the keyboard is up for one of the
                  editor's own fields. They are fixed in height, and under a
                  keyboard they left the settings -- the field being typed into
                  among them -- a strip about one line tall. Folded rather than
                  taken away, so the picture keeps its frame and the timeline
                  its zoom for when they come back. */}
              <motion.div
                variants={COLLAPSE}
                initial={false}
                animate={typing ? 'exit' : 'animate'}
                onAnimationComplete={(name) => {
                  if (name === 'exit') revealField();
                }}
                inert={typing}
                className="flex shrink-0 flex-col overflow-hidden"
              >
                <div
                  className="flex shrink-0 px-4"
                  style={{ height: `min(calc((100vw - 32px) * ${shape}), 36vh)` }}
                >
                  <PreviewStage
                    src={clip.previewSrc}
                    videoRef={videoRef}
                    aspect={options?.aspect ?? 'source'}
                    fit={options?.fit ?? 'fill'}
                    sourceWidth={clip.probe.width}
                    sourceHeight={clip.probe.height}
                    volume={options ? (options.mute ? 0 : options.volume) : 1}
                    playable={playable}
                    onUnplayable={markUnplayable}
                    onCappedChange={setPreviewCapped}
                    className="min-w-0 flex-1"
                  />
                </div>

                {/* -- the transport ----------------------------------------- */}
                <div className="flex h-14 shrink-0 items-center gap-1 px-2">
                  <TouchButton
                    label={playing ? t('editor.pause') : t('editor.play')}
                    // Nothing to play when the picture could not be decoded;
                    // the note under the preview says so.
                    disabled={running || !playable}
                    onClick={playPause}
                  >
                    {playing ? (
                      <Pause size={22} fill="currentColor" />
                    ) : (
                      <Play size={22} fill="currentColor" />
                    )}
                  </TouchButton>
                  <p className="tabular min-w-0 flex-1 truncate pl-1 text-[14px] text-fg">
                    <PlayheadTime playhead={playhead} format={formatTimecode} />
                    <span className="text-fg-faint"> / {formatTimecode(duration)}</span>
                  </p>
                  <TouchButton label={t('editor.split')} disabled={running} onClick={splitHere}>
                    <Scissors size={20} />
                  </TouchButton>
                  <TouchButton
                    label={t('editor.deleteSegment')}
                    disabled={running || cuts.length <= 1 || selected == null}
                    onClick={deleteSelected}
                  >
                    <Trash2 size={20} />
                  </TouchButton>
                </div>

                {/* -- the file, in time ------------------------------------- */}
                <div className="flex shrink-0 flex-col px-4 pb-1">
                  <TimelineDock
                    clipId={clip.id}
                    durationSec={duration}
                    fps={fps}
                    hasAudio={clip.probe.hasAudio}
                    cuts={cuts}
                    selectedId={selected?.id ?? null}
                    playhead={playhead}
                    playing={playing}
                    snapping={snapping}
                    keyframes={clip.keyframes}
                    losslessMode={losslessMode}
                    waveform={timeline.waveform}
                    filmstrip={timeline.filmstrip}
                    waveformWorking={timeline.working}
                    disabled={running}
                    onSeek={seek}
                    onSelect={setSelectedId}
                    onDragEdge={dragEdge}
                    onDragEdgeStart={beginGesture}
                    onDragEdgeEnd={endGesture}
                    onSplit={splitHere}
                    onDelete={deleteSelected}
                    onToggleSnapping={toggleSnapping}
                    onPlayPause={playPause}
                    onStep={step}
                    onViewChange={requestTimeline}
                    zoomRef={zoomRef}
                  />
                </div>
              </motion.div>

              {/* -- the settings, in what is left ------------------------- */}
              {/* A phone on its side is too short for the column above to
                  leave any of its height to the settings. There they take the
                  room they need, and the editor scrolls as a whole. */}
              <OutputInspector
                settings={settings}
                previewCapped={previewCapped}
                clips={clipStrip}
                className={cn(
                  'min-h-0 border-t border-[var(--border)] pt-2',
                  sideways ? 'flex-none' : 'flex-1',
                )}
              />
            </motion.div>
          ) : (
            <motion.div
              key="empty"
              ref={emptyRef}
              variants={EMPTY_SCREEN}
              custom={emptyScrolled}
              initial="initial"
              animate="animate"
              exit="exit"
              className="mx-auto w-full max-w-[760px] px-4 pb-12 pt-2"
            >
              {ffmpegNotice && <div className="mb-4">{ffmpegNotice}</div>}

              <div className="flex flex-col items-center rounded-[var(--radius-card)] border border-card-edge bg-surface px-6 py-9 text-center">
                <FilePlus2 size={28} strokeWidth={1.5} aria-hidden="true" className="text-fg-faint" />
                <p className="mt-3 text-[14px] font-medium text-fg">{t('editor.pickTitle')}</p>
                <p className="mt-0.5 text-[12.5px] text-fg-muted">{t('editor.pickBody')}</p>
                <div className="mt-4 flex items-center gap-2">
                  <Button
                    size="sm"
                    variant="secondary"
                    loading={picking || (opening && !openingRecent)}
                    disabled={opening}
                    onClick={addClip}
                  >
                    {t('editor.chooseVideo')}
                  </Button>
                  <Button size="sm" variant="ghost" icon={<Link2 size={15} />} onClick={openLink}>
                    {t('editor.linkTitle')}
                  </Button>
                </div>
              </div>

              {shownOpenError && (
                <InlineNotice tone="error" className="mt-2 px-4">
                  {shownOpenError}
                </InlineNotice>
              )}

              <RecentDownloads
                onOpen={openRecent}
                disabled={picking || opening}
                className="mt-6"
              />
            </motion.div>
          )}
        </AnimatePresence>

        {linkSheet}
        {discardQuestion}
      </>
    );
  }

  // -- nothing open --------------------------------------------------------

  if (!clip) {
    return (
      <div className="mx-auto w-full max-w-[760px] px-6 pb-12">
        <PageHeader title={t('editor.title')} />
        {ffmpegNotice && <div className="mb-5">{ffmpegNotice}</div>}

        <div
          className={cn(
            'flex flex-col items-center rounded-[var(--radius-card)] border border-card-edge px-6 py-9 text-center',
            DROP_TRANSITION,
            dragging ? 'bg-accent-soft ring-2 ring-accent' : 'bg-surface',
          )}
        >
          <FilePlus2 size={28} strokeWidth={1.5} aria-hidden="true" className="text-fg-faint" />
          <p className="mt-3 text-[14px] font-medium text-fg">{t('editor.emptyTitle')}</p>
          <p className="mt-0.5 text-[12.5px] text-fg-muted">{t('editor.emptyBody')}</p>
          <div className="mt-4 flex items-center gap-2">
            <Button
              size="sm"
              variant="secondary"
              loading={opening && !openingRecent}
              disabled={opening}
              onClick={addClip}
            >
              {t('editor.chooseFile')}
            </Button>
            <Button size="sm" variant="ghost" icon={<Link2 size={15} />} onClick={openLink}>
              {t('editor.linkTitle')}
            </Button>
          </div>
        </div>

        {shownOpenError && (
          <InlineNotice tone="error" className="mt-2 px-4">
            {shownOpenError}
          </InlineNotice>
        )}

        <RecentDownloads onOpen={openRecent} disabled={opening} className="mt-7" />

        {linkSheet}
      </div>
    );
  }

  // -- editing -------------------------------------------------------------

  return (
    <motion.div
      ref={surfaceRef}
      variants={EDITOR}
      initial="initial"
      animate="animate"
      tabIndex={-1}
      className={cn(
        // Exactly the window's height and never more: App gives this route a
        // screen of `h-full` rather than `min-h-full`, so growing into it and
        // `min-h-0` together cap the workspace, and a region taller than its
        // share -- the settings column, above all -- scrolls inside itself
        // instead of growing the page and pushing the dock off the bottom.
        'flex min-h-0 flex-1 flex-col outline-none',
        DROP_TRANSITION,
        dragging && 'ring-2 ring-inset ring-accent',
      )}
    >
      {/* -- the bar --------------------------------------------------------- */}
      <div className="flex h-[52px] shrink-0 items-center gap-3 border-b border-[var(--border)] px-4">
        <div className="flex min-w-0 flex-1 items-baseline gap-2">
          <p className="truncate text-[13.5px] font-medium text-fg">{clip.name}</p>
          <span className="tabular shrink-0 text-[12px] text-fg-faint">
            {formatTimecode(keptLength)} · {formatBytes(clip.probe.sizeBytes)}
          </span>
        </div>

        <div className="flex shrink-0 items-center gap-1">
          <IconButton
            size="sm"
            icon={<Undo2 size={15} />}
            label={`${t('editor.undo')} · Ctrl+Z`}
            disabled={!canUndo}
            onClick={undo}
          />
          <IconButton
            size="sm"
            icon={<Redo2 size={15} />}
            label={`${t('editor.redo')} · Ctrl+Shift+Z`}
            disabled={!canRedo}
            onClick={redo}
          />
          <span aria-hidden="true" className="mx-1 h-4 w-px bg-[var(--border)]" />
          <span className="tabular hidden text-[12px] text-fg-faint lg:inline">{summary}</span>
          {running ? (
            <Button size="sm" variant="secondary" onClick={() => void cancelExport()}>
              {t('common.cancel')}
            </Button>
          ) : (
            <Button
              size="sm"
              variant="primary"
              loading={submitting}
              disabled={!ffmpegReady}
              onClick={() => void startExport()}
            >
              {t('editor.export')}
            </Button>
          )}
          <IconButton
            size="sm"
            icon={<X size={15} />}
            label={t('editor.close')}
            onClick={requestClose}
          />
        </div>
      </div>

      {/* -- the stage ------------------------------------------------------- */}
      <div className="flex min-h-0 flex-1">
        <MediaRail
          clips={railClips}
          activeId={clip.id}
          onActivate={activate}
          onRemove={closeClip}
          onAdd={addClip}
          onAddLink={openLink}
        />

        <div className="flex min-w-0 flex-1 flex-col">
          <PreviewStage
            src={clip.previewSrc}
            videoRef={videoRef}
            aspect={options?.aspect ?? 'source'}
            fit={options?.fit ?? 'fill'}
            sourceWidth={clip.probe.width}
            sourceHeight={clip.probe.height}
            // What the export will sound like, removed audio included. M still
            // mutes the element on top of this, for the preview alone.
            volume={options ? (options.mute ? 0 : options.volume) : 1}
            playable={playable}
            onUnplayable={markUnplayable}
            onCappedChange={setPreviewCapped}
            className="min-h-0 flex-1"
          />

          <div className="flex h-7 shrink-0 items-center justify-between px-4">
            <span className="tabular text-[12px] text-fg-faint">
              <PlayheadTime playhead={playhead} format={framePosition} /> /{' '}
              {formatFrameTimecode(duration, fps)}
            </span>
            <span className="tabular text-[12px] text-fg-faint">
              {t('editor.length')} {formatFrameTimecode(keptLength, fps)}
            </span>
          </div>
        </div>

        <OutputInspector
          settings={settings}
          previewCapped={previewCapped}
          className="w-[272px] shrink-0 border-l border-[var(--border)] xl:w-[300px]"
        />
      </div>

      {/* -- the file, in time ----------------------------------------------- */}
      {/* Exactly as tall as what is in it -- 36 of tools, 24 of ruler, the two
          tracks and the overview bar. A taller dock would be empty space under
          the waveform, which reads as something that failed to load. */}
      <div className="flex h-[196px] shrink-0 flex-col border-t border-[var(--border)]">
        <TimelineDock
          clipId={clip.id}
          durationSec={duration}
          fps={fps}
          hasAudio={clip.probe.hasAudio}
          cuts={cuts}
          selectedId={selected?.id ?? null}
          playhead={playhead}
          playing={playing}
          snapping={snapping}
          keyframes={clip.keyframes}
          losslessMode={losslessMode}
          waveform={timeline.waveform}
          filmstrip={timeline.filmstrip}
          waveformWorking={timeline.working}
          disabled={running}
          onSeek={seek}
          onSelect={setSelectedId}
          onDragEdge={dragEdge}
          onDragEdgeStart={beginGesture}
          onDragEdgeEnd={endGesture}
          onSplit={splitHere}
          onDelete={deleteSelected}
          onToggleSnapping={toggleSnapping}
          onPlayPause={playPause}
          onStep={step}
          onViewChange={requestTimeline}
          zoomRef={zoomRef}
        />
      </div>

      {/* -- what the export said -------------------------------------------- */}
      <AnimatePresence initial={false}>
        {(running ||
          submitError ||
          shownOpenError ||
          job.status === 'failed' ||
          job.status === 'completed') && (
          <motion.div
            variants={COLLAPSE}
            initial="initial"
            animate="animate"
            exit="exit"
            className="shrink-0 overflow-hidden border-t border-[var(--border)]"
          >
            <div className="flex items-center gap-3 px-4 py-2.5">
              {/* A second file that would not open. The empty state is where
                  this is said with nothing open; with clips open it has no
                  other place, and a failed add that says nothing reads as a
                  button that did not work. Dismissed by hand, because it
                  belongs to no edit that could clear it. */}
              {shownOpenError && !running && (
                <>
                  <InlineNotice tone="error" className="flex-1">
                    {shownOpenError}
                  </InlineNotice>
                  <IconButton
                    size="sm"
                    icon={<X size={15} />}
                    label={t('common.close')}
                    onClick={() => {
                      setPickFailed(false);
                      useEditorStore.setState({ openError: null });
                    }}
                  />
                </>
              )}
              {running && <Progress value={job.percent} className="flex-1" label={t('editor.exporting')} />}

              {submitError && (
                <InlineNotice tone="error" className="flex-1">
                  {errorMessage(submitError)}
                </InlineNotice>
              )}
              {job.status === 'failed' && job.error && (
                <InlineNotice tone="error" className="flex-1">
                  {/* An export that failed for a reason with no code of its own
                      would otherwise be reported as a download that failed. */}
                  {job.error.code === 'unknown'
                    ? t('editor.exportFailed')
                    : errorMessage(job.error)}
                </InlineNotice>
              )}
              {job.status === 'completed' && job.outputPath && (
                <>
                  <div className="min-w-0 flex-1">
                    <p className="text-[13px] font-medium text-fg">{t('editor.exported')}</p>
                    <p className="mt-0.5 truncate text-[12px] text-fg-muted">{job.outputPath}</p>
                  </div>
                  <Button
                    size="sm"
                    variant="secondary"
                    onClick={() => void openFile(job.outputPath!)}
                  >
                    {t('common.open')}
                  </Button>
                  <IconButton
                    size="sm"
                    icon={<FolderOpen size={15} />}
                    label={t('downloads.showInFolder')}
                    onClick={() => void revealFile(job.outputPath!)}
                  />
                </>
              )}
            </div>
          </motion.div>
        )}
      </AnimatePresence>

      {linkSheet}
      {discardQuestion}
    </motion.div>
  );
}

/** Less than this lost from the viewport is a bar coming and going, not a keyboard. */
const KEYBOARD_MIN_PX = 120;

/**
 * The tallest the page has been at the width it has now. Kept for the page's
 * life rather than the editor's: the editor may first open with the keyboard
 * already up for something else, and measured then, a keyboard would read as
 * the screen's own height.
 */
const room = { width: 0, tallest: 0 };

/**
 * How the phone's screen stands around the editor: whether the keyboard is up
 * for a field inside `surfaceRef`, and whether the phone is on its side.
 *
 * Both halves of the first are asked. The focus alone is not enough: Android
 * leaves a field focused when the keyboard is put away with its own button or
 * with Back. Nor is the viewport alone: the link sheet's field brings the same
 * keyboard up over the editor, where the editor has nothing to make room for.
 *
 * The viewport is measured against the tallest it has been at its width --
 * the activity resizes the page for the keyboard -- and a new width is the
 * phone turned, a new screen and not a keyboard. On its side is judged from
 * that height too, not from the viewport's own shape: a tall keyboard leaves
 * an upright phone a strip wider than it is high, which is still upright.
 */
function usePhoneRoom(
  surfaceRef: RefObject<HTMLElement | null>,
  active: boolean,
): { typing: boolean; sideways: boolean } {
  const [focused, setFocused] = useState(false);
  const [short, setShort] = useState(false);
  const [sideways, setSideways] = useState(false);

  useEffect(() => {
    const surface = surfaceRef.current;
    if (!active || !surface) {
      setFocused(false);
      return;
    }
    const inside = (element: EventTarget | null) =>
      element instanceof Element && isTextField(element) && surface.contains(element);
    setFocused(inside(document.activeElement));
    const onIn = (event: FocusEvent) => setFocused(inside(event.target));
    // Where the focus is going, so a move from one field to another does not
    // unfold the picture for the moment in between.
    const onOut = (event: FocusEvent) => setFocused(inside(event.relatedTarget));
    surface.addEventListener('focusin', onIn);
    surface.addEventListener('focusout', onOut);
    return () => {
      surface.removeEventListener('focusin', onIn);
      surface.removeEventListener('focusout', onOut);
    };
  }, [active, surfaceRef]);

  useEffect(() => {
    if (!active) {
      setShort(false);
      setSideways(false);
      return;
    }
    const viewport = window.visualViewport;
    const measure = () => {
      const width = viewport ? viewport.width * viewport.scale : window.innerWidth;
      const height = viewport ? viewport.height * viewport.scale : window.innerHeight;
      if (Math.abs(width - room.width) > 1) {
        room.width = width;
        room.tallest = height;
      }
      room.tallest = Math.max(room.tallest, height);
      setShort(room.tallest - height >= KEYBOARD_MIN_PX);
      setSideways(width > room.tallest);
    };
    measure();
    const target = viewport ?? window;
    target.addEventListener('resize', measure);
    return () => target.removeEventListener('resize', measure);
  }, [active]);

  return { typing: focused && short, sideways };
}

/**
 * A round, fingertip-sized button with nothing but a glyph in it: the phone
 * editor's close, and its transport. Forty-four pixels, which is what a touch
 * target has to be, whatever the glyph inside it measures.
 */
function TouchButton({
  label,
  disabled = false,
  onClick,
  children,
}: {
  label: string;
  disabled?: boolean;
  onClick: () => void;
  children: ReactNode;
}) {
  return (
    <button
      type="button"
      aria-label={label}
      disabled={disabled}
      onClick={onClick}
      className={cn(
        'pressable-sm flex size-11 shrink-0 items-center justify-center rounded-full text-fg',
        'active:bg-surface-active disabled:pointer-events-none disabled:opacity-40',
      )}
    >
      {children}
    </button>
  );
}

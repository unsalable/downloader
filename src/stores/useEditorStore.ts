import { convertFileSrc } from '@tauri-apps/api/core';
import { create } from 'zustand';

import { translate } from '@/i18n';
import { basename } from '@/lib/format';
import {
  keptLength,
  moveEdge,
  removeCut,
  splitAt,
  toSegments,
  wholeFile,
  type Cut,
} from '@/lib/editor/segments';
import * as ipc from '@/services/ipc';
import type {
  AppErrorInfo,
  AspectRatio,
  ExportOptions,
  ExportState,
  FetchState,
  FrameFit,
  MediaProbe,
  TimelineState,
} from '@/types';

/**
 * Everything the editor is holding.
 *
 * Two things live here that the old Trim screen did not have, and both are the
 * reason it is a store rather than component state. A pool: several clips are
 * open at once and each keeps its own cuts and its own output settings, so
 * switching between them has to be free and lossless. And a history: once a
 * keystroke can delete a piece of someone's work, there has to be a way back,
 * and the cheapest honest one is a snapshot of the whole edit -- an edit is a
 * dozen numbers, and a list of reversible operations would be a second model of
 * the first one, kept in step by hand. Each clip keeps its own, for the same
 * reason it keeps its own cuts: going to look at another file is not a reason
 * to lose the way back through this one.
 */

const IDLE_EXPORT: ExportState = {
  status: 'idle',
  percent: null,
  outputPath: null,
  error: null,
};

const IDLE_FETCH: FetchState = {
  status: 'idle',
  percent: null,
  receivedBytes: 0,
  title: null,
  outputPath: null,
  error: null,
};

const IDLE_TIMELINE: TimelineState = {
  path: null,
  token: 0,
  working: false,
  waveform: null,
  filmstrip: null,
  error: null,
};

/** How many steps back the editor remembers. Past this it is a different edit. */
const HISTORY_DEPTH = 50;

/**
 * How long a run of changes to the same setting may pause and still be one
 * step. A slider reports every pixel it passes and a number field every
 * keystroke; the way back should hold the value the user left, not each one
 * they went through on the way to it.
 */
const RUN_MS = 700;

export interface Clip {
  id: string;
  path: string;
  name: string;
  probe: MediaProbe;
  /** What the picture element loads, once the file has been allowed. */
  previewSrc: string;
  /** Where a copied stream may begin. Null until it has been asked for. */
  keyframes: number[] | null;
}

/** The part of a clip the user can change, and therefore the part that undoes. */
interface Edit {
  cuts: Cut[];
  options: ExportOptions;
}

/** The way back through one clip's edit, and the way forward again. */
interface History {
  past: Edit[];
  future: Edit[];
}

/** Shared, so a clip with no steps yet reads as the same value every time. */
const NO_HISTORY: History = { past: [], future: [] };

/**
 * Whether two edits are the same edit, whatever objects they are made of.
 *
 * A step that changes nothing is worse than no step at all: it is an undo that
 * appears to do nothing, which is exactly how people conclude that undo is
 * broken. The helpers try to hand back what they were given when they refuse,
 * but a picker re-choosing its own value, or an edge dropped where it already
 * was, arrives here as a new object with the old numbers in it -- so the
 * numbers are what is compared.
 */
function sameEdit(a: Edit, b: Edit): boolean {
  if (a === b) return true;
  if (a.cuts !== b.cuts) {
    if (a.cuts.length !== b.cuts.length) return false;
    for (let index = 0; index < a.cuts.length; index += 1) {
      const x = a.cuts[index]!;
      const y = b.cuts[index]!;
      if (x.id !== y.id || x.startSec !== y.startSec || x.endSec !== y.endSec) return false;
    }
  }
  if (a.options !== b.options) {
    const keys = new Set([...Object.keys(a.options), ...Object.keys(b.options)]);
    for (const key of keys) {
      if (a.options[key as keyof ExportOptions] !== b.options[key as keyof ExportOptions]) {
        return false;
      }
    }
  }
  return true;
}

function defaultOptions(probe: MediaProbe, hardware: boolean): ExportOptions {
  // The source's own container, so the quiet path through this screen is a cut
  // and nothing else. Anything the app cannot copy into is landed in MP4 -- and
  // landing it somewhere else is already the decision to re-encode, so the mode
  // says so rather than leaving the screen to contradict itself.
  const copyable = ['mp4', 'mkv', 'webm', 'mov'].includes(probe.container);
  return {
    mode: copyable ? 'lossless' : 'reencode',
    container: copyable ? probe.container : 'mp4',
    videoCodec: 'h264',
    quality: 'balanced',
    videoBitrateKbps: null,
    fps: null,
    maxHeight: null,
    aspect: 'source',
    fit: 'fill',
    mute: false,
    volume: 1,
    audioCodec: 'aac',
    audioBitrateKbps: 192,
    toneMapSdr: false,
    hardware,
  };
}

/**
 * A refusal the app itself decided on, in the shape the error surfaces take.
 * The code is one no dictionary answers to, so what is shown is the sentence
 * below rather than a stock one about downloads.
 */
function ownError(message: string, technical: string | null = null): AppErrorInfo {
  return { code: 'editorRefused', title: message, message, technical, retryable: false };
}

let clipCounter = 0;

interface EditorStoreState {
  pool: Clip[];
  activeId: string | null;
  edits: Record<string, Edit>;

  opening: boolean;
  openError: AppErrorInfo | null;

  /** Each open clip's way back, by clip id. A clip with no steps has no entry. */
  history: Record<string, History>;

  job: ExportState;
  submitting: boolean;
  submitError: AppErrorInfo | null;

  fetch: FetchState;
  timeline: TimelineState;

  /**
   * Where exports land. Null means the default: beside the source file, or the
   * download folder for a clip that is one of the app's own (see
   * `ipc.exportDefaultDir`).
   */
  outputDir: string | null;

  open: (path: string, hardware: boolean) => Promise<string | null>;
  activate: (id: string) => void;
  closeClip: (id: string) => void;
  closeAll: () => void;

  split: (seconds: number) => void;
  remove: (id: string) => void;
  /** `held` when the move comes from a pointer that is still down. */
  drag: (id: string, edge: 'start' | 'end', seconds: number, held?: boolean) => void;
  setOptions: (patch: Partial<ExportOptions>) => void;
  setAspect: (aspect: AspectRatio, fit: FrameFit) => void;
  setOutputDir: (dir: string | null) => void;

  /**
   * Hold the way back while a pointer drags: everything the drag does between
   * these two is one step, recorded when it ends -- and none at all if the
   * edge was let go of where it started.
   */
  beginGesture: () => void;
  endGesture: () => void;

  undo: () => void;
  redo: () => void;

  loadKeyframes: () => Promise<void>;
  startExport: () => Promise<void>;
  cancelExport: () => Promise<void>;
  cancelFetch: () => Promise<void>;

  applyExport: (state: ExportState) => void;
  applyFetch: (state: FetchState) => void;
  applyTimeline: (state: TimelineState) => void;
}

export const useEditorStore = create<EditorStoreState>((set, get) => {
  // Two pieces of bookkeeping that no screen draws, so they live beside the
  // store rather than in it: a change to either would wake every subscriber
  // for nothing.
  //
  // A drag that is still held: the edit as it stood when the pointer went
  // down. The moves change the edit as they arrive, but the way back is
  // written once, when the pointer lets go.
  let gesture: { clipId: string; before: Edit } | null = null;
  // The run of changes a step is still gathering: which clip, which settings,
  // and when it last grew. Anything else that touches the way back ends it.
  let run: { clipId: string; key: string; at: number } | null = null;

  /** The way back for `clipId` with `before` on top, and no way forward. */
  const pushed = (clipId: string, before: Edit): Record<string, History> => {
    const { history } = get();
    const own = history[clipId] ?? NO_HISTORY;
    return {
      ...history,
      [clipId]: { past: [...own.past, before].slice(-HISTORY_DEPTH), future: [] },
    };
  };

  /**
   * What a change to the edit, in either direction, does to the rest of the
   * screen: it is the answer to whatever the last finished export said. An
   * export still running is working from the edit it was given, and stays.
   */
  const answered = () => ({
    submitError: null,
    job: get().job.status === 'running' ? get().job : IDLE_EXPORT,
  });

  /**
   * Close a drag that is still held, keeping it as the one step it is.
   *
   * Everything that is not the drag itself calls this first. A drag whose
   * release never arrived -- the pointer let go over another window, the dock
   * went away under it -- would otherwise go on quietly swallowing every step
   * after it.
   */
  const settle = () => {
    const held = gesture;
    gesture = null;
    if (!held) return;
    const now = get().edits[held.clipId];
    if (!now || sameEdit(now, held.before)) return;
    run = null;
    set({ history: pushed(held.clipId, held.before) });
  };

  /**
   * Change the active clip's edit, keeping the way back.
   *
   * Every mutation goes through here, so nothing can change the edit without
   * being undoable -- which is the sort of rule that only holds if there is
   * exactly one door.
   *
   * Most changes are a step of their own. Two kinds are not: a move made while
   * a drag is held belongs to the drag, and is recorded when it ends; and a
   * change that names a `run` folds into the step before it when that step was
   * the same run, still going -- the same key, on the same clip, within
   * RUN_MS of its last beat.
   */
  const commit = (
    change: (edit: Edit) => Edit,
    how: { drag?: boolean; held?: boolean; run?: string } = {},
  ) => {
    if (!how.drag) settle();
    if (!how.run) run = null;

    const { activeId, edits } = get();
    if (!activeId) return;
    const current = edits[activeId];
    if (!current) return;
    const next = change(current);
    if (sameEdit(next, current)) return;

    const changed = { edits: { ...edits, [activeId]: next }, ...answered() };

    // A pointer still down on a drag that something else closed half-way -- an
    // undo or a split pressed without letting go -- goes on as one more step,
    // not as one for every move it makes before it is released.
    if (how.held && !gesture) gesture = { clipId: activeId, before: current };

    if (gesture?.clipId === activeId) {
      set(changed);
      return;
    }

    const now = Date.now();
    const own = get().history[activeId] ?? NO_HISTORY;
    const start = own.past[own.past.length - 1];
    if (
      how.run &&
      run &&
      start &&
      run.clipId === activeId &&
      run.key === how.run &&
      now - run.at <= RUN_MS
    ) {
      if (sameEdit(next, start)) {
        // Back where the run began: the step it made has nothing left in it.
        run = null;
        set({
          ...changed,
          history: { ...get().history, [activeId]: { past: own.past.slice(0, -1), future: [] } },
        });
        return;
      }
      run.at = now;
      set(changed);
      return;
    }

    run = how.run ? { clipId: activeId, key: how.run, at: now } : null;
    set({ ...changed, history: pushed(activeId, current) });
  };

  return {
    pool: [],
    activeId: null,
    edits: {},

    opening: false,
    openError: null,

    history: {},

    job: IDLE_EXPORT,
    submitting: false,
    submitError: null,

    fetch: IDLE_FETCH,
    timeline: IDLE_TIMELINE,
    outputDir: null,

    open: async (path, hardware) => {
      // The same file twice is the same clip: opening it again should take the
      // user to the edit they already have, not start a second one beside it.
      const existing = get().pool.find((clip) => clip.path === path);
      if (existing) {
        get().activate(existing.id);
        return existing.id;
      }

      set({ opening: true, openError: null });
      try {
        const probe = await ipc.probeMedia(path);
        // Sound on its own is not something this screen can cut: it would
        // open as a black stage over an empty filmstrip. The phone's picker
        // offers audio as well, because the same picker serves conversions,
        // and a file dropped on the window can be anything -- so the refusal
        // is made here, once, for every way in.
        if (!probe.hasVideo) {
          set({ opening: false, openError: ownError(translate('editor.noVideo')) });
          return null;
        }
        // Some files carry no length at all -- anything muxed to a pipe, which
        // is how a lot of captures are written. Opening one anyway would mean an
        // edit a twentieth of a second long, and an export that quietly wrote
        // four frames of it, so the honest answer is to say what is missing.
        if (probe.durationSec == null || probe.durationSec <= 0) {
          set({ opening: false, openError: ownError(translate('editor.unknownLength')) });
          return null;
        }
        // The webview may read this one file, and only from here on. Without it
        // the asset protocol refuses the request and the picture stays blank.
        await ipc.allowMediaPreview(path);

        clipCounter += 1;
        const id = `clip-${clipCounter}`;
        const clip: Clip = {
          id,
          path,
          name: basename(path),
          probe,
          previewSrc: convertFileSrc(path),
          keyframes: null,
        };
        const duration = probe.durationSec ?? 0;

        // The clip being left keeps its way back; a drag still held on it is
        // closed as the step it is. The new one starts with none.
        settle();
        run = null;
        set((state) => ({
          pool: [...state.pool, clip],
          edits: {
            ...state.edits,
            [id]: { cuts: wholeFile(duration), options: defaultOptions(probe, hardware) },
          },
          activeId: id,
          opening: false,
          job: IDLE_EXPORT,
          submitError: null,
          // Drawn from the clip that was open a moment ago, and about to be
          // painted under this one's ruler until the backend answers.
          timeline: IDLE_TIMELINE,
        }));
        return id;
      } catch (caught) {
        // Said in the editor's own words, whatever the backend's reason. The
        // shared dictionary answers a failure by its code, and every sentence
        // it has is about a download -- one that "could not be completed", a
        // download folder to change -- when what failed here was a file the
        // user asked to open. What went wrong underneath still travels with it,
        // as the technical detail, for anything that wants to say more.
        const cause = ipc.toAppError(caught);
        set({
          opening: false,
          openError: ownError(translate('editor.openFailed'), cause.technical ?? cause.message),
        });
        return null;
      }
    },

    activate: (id) => {
      if (get().activeId === id) return;
      // History belongs to the clip it was made on, and stays with it: an undo
      // here can only ever reach this clip's edit, and coming back to the one
      // being left finds its way back where it was. The waveform and the
      // frames are different -- they are drawn for one file and are visible,
      // so they go.
      settle();
      run = null;
      set({ activeId: id, submitError: null, timeline: IDLE_TIMELINE });
    },

    closeClip: (id) => {
      // A drag held on the clip that is going has nothing left to record.
      if (gesture?.clipId === id) gesture = null;
      if (run?.clipId === id) run = null;
      set((state) => {
        const pool = state.pool.filter((clip) => clip.id !== id);
        const edits = { ...state.edits };
        delete edits[id];
        const history = { ...state.history };
        delete history[id];
        // Removing some other clip is not a clip switch: the one being edited,
        // and the way back through everything done to it, are untouched.
        if (state.activeId !== id) return { pool, edits, history };
        return {
          pool,
          edits,
          history,
          activeId: pool[pool.length - 1]?.id ?? null,
          timeline: IDLE_TIMELINE,
        };
      });
    },

    closeAll: () => {
      if (get().job.status === 'running') void ipc.cancelExport();
      gesture = null;
      run = null;
      set({
        pool: [],
        edits: {},
        activeId: null,
        history: {},
        job: IDLE_EXPORT,
        submitError: null,
        openError: null,
        timeline: IDLE_TIMELINE,
      });
    },

    // Both of these can be refused -- a split at the edge of a piece, a delete
    // of the last one -- and a refusal has to reach `commit` as the edit it was
    // handed, or the way back fills up with steps that changed nothing.
    split: (seconds) =>
      commit((edit) => {
        const cuts = splitAt(edit.cuts, seconds);
        return cuts === edit.cuts ? edit : { ...edit, cuts };
      }),

    remove: (id) =>
      commit((edit) => {
        const cuts = removeCut(edit.cuts, id);
        return cuts === edit.cuts ? edit : { ...edit, cuts };
      }),

    // A step of its own from the keyboard; one beat of a held drag from the
    // pointer, which the drag records as a whole when it ends.
    drag: (id, edge, seconds, held = false) => {
      const clip = selectActiveClip(get());
      const duration = clip?.probe.durationSec ?? 0;
      commit(
        (edit) => {
          const cuts = moveEdge(edit.cuts, id, edge, seconds, duration);
          return cuts === edit.cuts ? edit : { ...edit, cuts };
        },
        { drag: true, held },
      );
    },

    // A slider reports as it moves and a number field as it is typed in, so a
    // run of patches to the same settings folds into one step. The run is named
    // by which settings it touches: a different set is a different decision.
    setOptions: (patch) => {
      const probe = selectActiveClip(get())?.probe ?? null;
      commit(
        (edit) => ({
          ...edit,
          options: withLegalMode({ ...edit.options, ...patch }, probe),
        }),
        { run: `options:${Object.keys(patch).sort().join(',')}` },
      );
    },

    // Reshaping the frame means decoding it. Saying so in the same change
    // rather than refusing later is what keeps the two controls from
    // disagreeing -- and it is the same rule as every other option's.
    setAspect: (aspect, fit) => {
      const probe = selectActiveClip(get())?.probe ?? null;
      commit((edit) => ({
        ...edit,
        options: withLegalMode({ ...edit.options, aspect, fit }, probe),
      }));
    },

    setOutputDir: (outputDir) => set({ outputDir }),

    beginGesture: () => {
      settle();
      run = null;
      const { activeId, edits } = get();
      const current = activeId ? edits[activeId] : undefined;
      if (!activeId || !current) return;
      gesture = { clipId: activeId, before: current };
    },

    endGesture: () => settle(),

    // A drag still held when one of these arrives is closed first, as its own
    // step, so the way back is never walked out from under it.
    undo: () => {
      settle();
      run = null;
      const { activeId, edits, history } = get();
      if (!activeId) return;
      const current = edits[activeId];
      const own = history[activeId];
      if (!current || !own || own.past.length === 0) return;
      const previous = own.past[own.past.length - 1]!;
      set({
        edits: { ...edits, [activeId]: previous },
        history: {
          ...history,
          [activeId]: {
            past: own.past.slice(0, -1),
            future: [current, ...own.future].slice(0, HISTORY_DEPTH),
          },
        },
        ...answered(),
      });
    },

    redo: () => {
      settle();
      run = null;
      const { activeId, edits, history } = get();
      if (!activeId) return;
      const current = edits[activeId];
      const own = history[activeId];
      if (!current || !own || own.future.length === 0) return;
      const next = own.future[0]!;
      set({
        edits: { ...edits, [activeId]: next },
        history: {
          ...history,
          [activeId]: {
            past: [...own.past, current].slice(-HISTORY_DEPTH),
            future: own.future.slice(1),
          },
        },
        ...answered(),
      });
    },

    loadKeyframes: async () => {
      const clip = selectActiveClip(get());
      if (!clip || clip.keyframes) return;
      try {
        const keyframes = await ipc.mediaKeyframes(clip.path);
        set((state) => ({
          pool: state.pool.map((entry) =>
            entry.id === clip.id ? { ...entry, keyframes } : entry,
          ),
        }));
      } catch {
        // A file whose keyframes cannot be read is still perfectly cuttable;
        // the interface simply cannot show where a copied cut would land, and
        // an empty list says exactly that.
        set((state) => ({
          pool: state.pool.map((entry) =>
            entry.id === clip.id ? { ...entry, keyframes: [] } : entry,
          ),
        }));
      }
    },

    startExport: async () => {
      const state = get();
      const clip = selectActiveClip(state);
      const edit = state.activeId ? state.edits[state.activeId] : undefined;
      if (!clip || !edit || state.submitting) return;

      set({ submitting: true, submitError: null });
      try {
        await ipc.startExport({
          inputPath: clip.path,
          segments: toSegments(edit.cuts),
          options: edit.options,
          outputDir: state.outputDir,
        });
      } catch (caught) {
        set({ submitError: ipc.toAppError(caught) });
      } finally {
        set({ submitting: false });
      }
    },

    cancelExport: async () => {
      try {
        await ipc.cancelExport();
      } catch {
        // It either stopped or was already over; either way there is nothing
        // here the user could act on.
      }
    },

    cancelFetch: async () => {
      try {
        await ipc.cancelRangeFetch();
      } catch {
        // As above.
      }
    },

    applyExport: (job) => set({ job }),
    applyFetch: (fetch) => set({ fetch }),
    applyTimeline: (timeline) => {
      // A redraw that belongs to a clip the user has already left is not an
      // error, it is simply late. Dropping it is cheaper than cancelling.
      //
      // Decided before `set` rather than inside it: an updater that returns the
      // state it was handed still produces a new state object, every subscriber
      // still wakes, and a screen that redraws on every stale event is the
      // shape an infinite loop takes here.
      const clip = selectActiveClip(get());
      if (timeline.path && clip && timeline.path !== clip.path) return;
      set({ timeline });
    },
  };
});

export function selectActiveClip(state: EditorStoreState): Clip | null {
  return state.pool.find((clip) => clip.id === state.activeId) ?? null;
}

/**
 * Whether anything is open in the editor. On a phone this is what hands the
 * editor the whole screen -- the shell's bars step aside, and the Back gesture
 * closes the clip rather than leaving the tab -- so the shell reads it here
 * instead of asking the page.
 */
export function selectEditing(state: EditorStoreState): boolean {
  return state.pool.length > 0;
}

/**
 * One shared empty list, because a selector that builds its own falls foul of
 * the rule that makes subscriptions work: a fresh array every call is a new
 * value every call, so the component re-renders, the selector runs again, and
 * the two feed each other until React gives up.
 */
const NO_CUTS: Cut[] = [];

export function selectCuts(state: EditorStoreState): Cut[] {
  return (state.activeId ? state.edits[state.activeId]?.cuts : undefined) ?? NO_CUTS;
}

export function selectOptions(state: EditorStoreState): ExportOptions | null {
  return (state.activeId ? state.edits[state.activeId]?.options : undefined) ?? null;
}

/** Seconds that would be written, which is not the span the pieces cover. */
export function selectKeptLength(state: EditorStoreState): number {
  return keptLength(selectCuts(state));
}

/** The way back and forward through the clip being edited. */
export function selectHistory(state: EditorStoreState): History {
  return (state.activeId ? state.history[state.activeId] : undefined) ?? NO_HISTORY;
}

/**
 * Whether any open clip has a step to take back -- which is what closing them
 * all would throw away, whichever of them happens to be on screen.
 */
export function selectAnyHistory(state: EditorStoreState): boolean {
  return Object.values(state.history).some((own) => own.past.length > 0);
}

/**
 * Why a lossless export is not on offer, as the name of the setting that ruled
 * it out -- or null when it is available.
 *
 * Expressed as the cause rather than as a boolean because the interface has to
 * say which choice took it away; "lossless is unavailable" with no reason is
 * the kind of message people go looking through settings over.
 */
export function selectLosslessBlocker(state: EditorStoreState): LosslessBlocker | null {
  const options = selectOptions(state);
  const clip = selectActiveClip(state);
  if (!options || !clip) return null;
  return losslessBlocker(options, clip.probe);
}

export type LosslessBlocker =
  | 'aspect'
  | 'fps'
  | 'resolution'
  | 'volume'
  | 'colour'
  | 'container';

/**
 * The rule behind `selectLosslessBlocker`, for any options and any source.
 *
 * Everything here changes what a stream copy cannot: the frame's shape, rate
 * or size, the audio's level, the colours, or the box it is written into. A
 * level within half a percent of the source's is the source's -- a slider
 * parked on 100 must not rule anything out over rounding. Removing the audio is
 * not here: leaving a track out decodes nothing, so a copy does it too, and a
 * level on a track that is not written rules nothing out. The backend's
 * `lossless_is_possible` holds the same rule.
 */
export function losslessBlocker(options: ExportOptions, probe: MediaProbe): LosslessBlocker | null {
  if (options.aspect !== 'source') return 'aspect';
  if (options.fps != null) return 'fps';
  if (options.maxHeight != null) return 'resolution';
  const audible = probe.hasAudio && !options.mute;
  if (audible && Math.abs(options.volume - 1) > 0.005) return 'volume';
  if (options.toneMapSdr) return 'colour';
  if (probe.container !== options.container) return 'container';
  return null;
}

/**
 * The options, with a mode that agrees with them.
 *
 * A change that rules out a lossless copy is a change to re-encode, made in
 * the same step: the alternative is a screen whose mode says one thing and
 * whose settings say another until the export refuses. Only ever moves one
 * way -- putting the setting back leaves the mode where the user can see it,
 * rather than switching it behind their back a second time.
 */
export function withLegalMode(options: ExportOptions, probe: MediaProbe | null): ExportOptions {
  if (options.mode !== 'lossless' || !probe) return options;
  return losslessBlocker(options, probe) == null ? options : { ...options, mode: 'reencode' };
}

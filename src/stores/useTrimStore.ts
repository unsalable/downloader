import { convertFileSrc } from '@tauri-apps/api/core';
import { create } from 'zustand';

import * as ipc from '@/services/ipc';
import type { MediaProbe, TrimPrecision, TrimState } from '@/types';

/** Nothing has been asked for yet. */
const IDLE: TrimState = { status: 'idle', percent: null, outputPath: null, error: null };

/**
 * The shortest cut the screen will offer. Matches the floor the backend
 * enforces, so the button goes quiet before the request would be refused.
 */
export const MIN_TRIM_SEC = 0.05;

function clamp(value: number, low: number, high: number): number {
  if (!Number.isFinite(value)) return low;
  return Math.min(Math.max(value, low), Math.max(low, high));
}

/** Moving a mark is the answer to whatever the last finished cut said. */
function settle(state: TrimStoreState) {
  return {
    submitError: null,
    job: state.job.status === 'running' ? state.job : IDLE,
  };
}

interface TrimStoreState {
  /** The file that is open, once it has been read. */
  probe: MediaProbe | null;
  /** What the `<video>` element loads. Null until a file has been allowed. */
  previewSrc: string | null;
  opening: boolean;
  openError: string | null;

  /** The marks, in seconds from the start of the file. */
  startSec: number;
  endSec: number;
  precision: TrimPrecision;
  outputDir: string | null;

  /** What the backend last said about the cut itself. */
  job: TrimState;
  submitting: boolean;
  submitError: string | null;

  open: (path: string) => Promise<void>;
  close: () => void;
  setStart: (seconds: number) => void;
  setEnd: (seconds: number) => void;
  setPrecision: (precision: TrimPrecision) => void;
  setOutputDir: (dir: string | null) => void;
  start: () => Promise<void>;
  cancel: () => Promise<void>;
  /** From the `trim://changed` event. */
  apply: (state: TrimState) => void;
}

export const useTrimStore = create<TrimStoreState>((set, get) => ({
  probe: null,
  previewSrc: null,
  opening: false,
  openError: null,

  startSec: 0,
  endSec: 0,
  precision: 'fast',
  outputDir: null,

  job: IDLE,
  submitting: false,
  submitError: null,

  open: async (path) => {
    set({ opening: true, openError: null, submitError: null, job: IDLE });
    try {
      const probe = await ipc.probeMedia(path);
      // The webview may read this one file, and only from here on. Without it
      // the asset protocol refuses the request and the video stays blank.
      await ipc.allowMediaPreview(path);
      set({
        probe,
        previewSrc: convertFileSrc(path),
        // A file opens whole: the marks are the file until they are moved.
        startSec: 0,
        endSec: probe.durationSec ?? 0,
        opening: false,
      });
    } catch (caught) {
      set({
        probe: null,
        previewSrc: null,
        opening: false,
        openError: ipc.toAppError(caught).message,
      });
    }
  },

  close: () => {
    // A cut still running belongs to the file being closed.
    if (get().job.status === 'running') void ipc.cancelTrim();
    set({
      probe: null,
      previewSrc: null,
      openError: null,
      startSec: 0,
      endSec: 0,
      outputDir: null,
      job: IDLE,
      submitError: null,
    });
  },

  // Each mark moves on its own, clamped against whatever the other one is at
  // the moment it lands. Taking both at once looks tidier and is a bug: two
  // updates in the same frame make the second one carry a stale copy of the
  // first one's mark, and quietly put it back.
  setStart: (seconds) =>
    set((state) => ({
      startSec: clamp(seconds, 0, Math.max(0, state.endSec - MIN_TRIM_SEC)),
      ...settle(state),
    })),

  setEnd: (seconds) =>
    set((state) => {
      const duration = state.probe?.durationSec ?? 0;
      return {
        endSec: clamp(seconds, Math.min(state.startSec + MIN_TRIM_SEC, duration), duration),
        ...settle(state),
      };
    }),

  setPrecision: (precision) => set({ precision }),
  setOutputDir: (outputDir) => set({ outputDir }),

  start: async () => {
    const { probe, startSec, endSec, precision, outputDir, submitting } = get();
    if (!probe || submitting) return;
    if (endSec - startSec < MIN_TRIM_SEC) return;

    set({ submitting: true, submitError: null });
    try {
      await ipc.startTrim({
        inputPath: probe.path,
        startSec,
        endSec,
        precision,
        outputDir,
      });
    } catch (caught) {
      set({ submitError: ipc.toAppError(caught).message });
    } finally {
      set({ submitting: false });
    }
  },

  cancel: async () => {
    try {
      await ipc.cancelTrim();
    } catch {
      // The cut either stopped or was already over; either way there is
      // nothing here the user could act on.
    }
  },

  apply: (job) => set({ job }),
}));

/** The length of what would be written, in seconds. */
export function selectTrimLength(state: TrimStoreState): number {
  return Math.max(0, state.endSec - state.startSec);
}

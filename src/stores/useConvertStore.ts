import { create } from 'zustand';

import * as ipc from '@/services/ipc';
import type { ConvertJob, ConvertOptions, MediaProbe } from '@/types';

/**
 * A file the user has added but not yet converted.
 *
 * Probing happens as soon as a file is picked, so the list can show what each
 * one actually is -- and refuse an unreadable file straight away rather than
 * after it reaches the front of the queue.
 */
export interface StagedFile {
  path: string;
  name: string;
  probe: MediaProbe | null;
  /** Set when the file could not be read; the row is shown greyed out. */
  error: string | null;
  probing: boolean;
}

interface ConvertState {
  jobs: ConvertJob[];
  files: StagedFile[];
  loaded: boolean;
  submitting: boolean;

  load: () => Promise<void>;
  replace: (jobs: ConvertJob[]) => void;
  applyProgress: (event: ipc.ConvertProgressEvent) => void;

  addFiles: (paths: string[]) => Promise<void>;
  removeFile: (path: string) => void;
  clearFiles: () => void;
  submit: (options: ConvertOptions) => Promise<{ queued: number; error: string | null }>;
}

export const useConvertStore = create<ConvertState>((set, get) => ({
  jobs: [],
  files: [],
  loaded: false,
  submitting: false,

  load: async () => {
    const jobs = await ipc.listConversions();
    set({ jobs, loaded: true });
  },

  replace: (jobs) => set({ jobs, loaded: true }),

  /**
   * Progress arrives several times a second for the running job. Only that row
   * is replaced, and the array identity is preserved when nothing matched, so a
   * stale event cannot re-render the whole list.
   */
  applyProgress: (event) =>
    set((state) => {
      const index = state.jobs.findIndex((job) => job.id === event.id);
      if (index === -1) return state;

      const existing = state.jobs[index]!;
      const next = state.jobs.slice();
      next[index] = {
        ...existing,
        status: event.status,
        percent: event.percent,
        outputPath: event.outputPath ?? existing.outputPath,
        outputSizeBytes: event.outputSizeBytes ?? existing.outputSizeBytes,
        streamCopied: event.streamCopied,
        error: event.error,
      };
      return { jobs: next };
    }),

  addFiles: async (paths) => {
    const known = new Set(get().files.map((file) => file.path));
    const fresh = paths.filter((path) => path.trim() !== '' && !known.has(path));
    if (fresh.length === 0) return;

    set((state) => ({
      files: [
        ...state.files,
        ...fresh.map((path) => ({
          path,
          name: basename(path),
          probe: null,
          error: null,
          probing: true,
        })),
      ],
    }));

    // Probed one at a time: each is a short subprocess, and a folder full of
    // files dropped at once should not launch fifty of them together.
    for (const path of fresh) {
      try {
        const probe = await ipc.probeMedia(path);
        set((state) => ({
          files: state.files.map((file) =>
            file.path === path ? { ...file, probe, probing: false, name: probe.fileName } : file,
          ),
        }));
      } catch (error) {
        const message = ipc.toAppError(error).message;
        set((state) => ({
          files: state.files.map((file) =>
            file.path === path ? { ...file, error: message, probing: false } : file,
          ),
        }));
      }
    }
  },

  removeFile: (path) =>
    set((state) => ({ files: state.files.filter((file) => file.path !== path) })),

  clearFiles: () => set({ files: [] }),

  submit: async (options) => {
    const ready = get().files.filter((file) => file.error === null && !file.probing);
    if (ready.length === 0 || get().submitting) return { queued: 0, error: null };

    set({ submitting: true });
    try {
      await ipc.enqueueConversions({
        inputPaths: ready.map((file) => file.path),
        options,
      });
      // The staging list is emptied on success only: a rejected request should
      // leave the user's selection intact so they can change an option and try
      // again.
      set({ files: [] });
      return { queued: ready.length, error: null };
    } catch (error) {
      return { queued: 0, error: ipc.toAppError(error).message };
    } finally {
      set({ submitting: false });
    }
  },
}));

function basename(path: string): string {
  const parts = path.split(/[\\/]/);
  return parts[parts.length - 1] || path;
}

export function selectActiveJobs(jobs: ConvertJob[]): ConvertJob[] {
  return jobs.filter((job) => job.status === 'queued' || job.status === 'running');
}

export function selectFinishedJobs(jobs: ConvertJob[]): ConvertJob[] {
  return jobs.filter(
    (job) =>
      job.status === 'completed' || job.status === 'failed' || job.status === 'canceled',
  );
}

/** Count used by the sidebar badge. */
export function selectConvertInFlight(jobs: ConvertJob[]): number {
  return selectActiveJobs(jobs).length;
}

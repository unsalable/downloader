import { create } from 'zustand';

import * as ipc from '@/services/ipc';
import type { AppErrorInfo, AppUpdate, UpdateProgress } from '@/types';

/**
 * `ready` means the system installer was opened. Android replaces the app from
 * there; if the user backs out instead, the prompt stays up to open it again.
 */
export type UpdatePhase = 'idle' | 'downloading' | 'ready' | 'failed';

interface UpdateState {
  update: AppUpdate | null;
  open: boolean;
  phase: UpdatePhase;
  progress: UpdateProgress | null;
  error: AppErrorInfo | null;
  lastCheckedAt: number;
  /** The build the user said "later" to, which is not offered again unasked. */
  postponed: string | null;
  /**
   * Look for a newer build. An automatic check stays silent about failures --
   * being offline is no reason to interrupt anyone -- and does not re-offer a
   * build the user postponed. A manual one does both.
   */
  check: (manual?: boolean) => Promise<AppUpdate | null>;
  install: () => Promise<void>;
  postpone: () => void;
  setProgress: (progress: UpdateProgress) => void;
}

export const useUpdateStore = create<UpdateState>((set, get) => ({
  update: null,
  open: false,
  phase: 'idle',
  progress: null,
  error: null,
  lastCheckedAt: 0,
  postponed: null,

  check: async (manual = false) => {
    if (get().phase === 'downloading') return get().update;
    set({ lastCheckedAt: Date.now() });

    let found: AppUpdate | null;
    try {
      found = await ipc.checkAppUpdate();
    } catch (caught) {
      if (manual) throw caught;
      return null;
    }

    if (!found) {
      set({ update: null });
      return null;
    }
    const offer = manual || found.commit !== get().postponed;
    set((state) => ({
      update: found,
      open: state.open || offer,
      // A different build replaces whatever state the previous one was in.
      ...(state.update?.commit !== found.commit
        ? { phase: 'idle' as const, error: null, progress: null }
        : {}),
    }));
    return found;
  },

  install: async () => {
    const update = get().update;
    if (!update || get().phase === 'downloading') return;
    set({ phase: 'downloading', progress: null, error: null });
    try {
      await ipc.installAppUpdate(update);
      set({ phase: 'ready' });
    } catch (caught) {
      set({ phase: 'failed', error: ipc.toAppError(caught) });
    }
  },

  postpone: () => {
    if (get().phase === 'downloading') return;
    set((state) => ({ open: false, postponed: state.update?.commit ?? null }));
  },

  setProgress: (progress) => set({ progress }),
}));

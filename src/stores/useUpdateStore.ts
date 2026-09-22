import { create } from 'zustand';

import { IS_MOBILE } from '@/lib/platform';
import * as ipc from '@/services/ipc';
import type { AppErrorInfo, AppUpdate, UpdateProgress } from '@/types';

/**
 * `ready` means the download is done with. On the phone the system installer
 * was opened: Android replaces the app from there, and if the user backs out
 * instead, the prompt stays up to open it again. On the desktop the installer
 * is staged and verified, and waits for `apply`; `applying` is the moment
 * between starting it and the app going away.
 */
export type UpdatePhase = 'idle' | 'downloading' | 'ready' | 'applying' | 'failed';

const ATTEMPT_KEY = 'ud.update.attempt';
const ATTEMPT_COOLDOWN_MS = 24 * 60 * 60 * 1000;

/**
 * The desktop installs without asking, so an installer that keeps failing -- a
 * declined elevation prompt, a file it cannot replace -- would close the app
 * at every launch. A build that is still on offer after its installer ran is
 * one that did not take, so by itself the app tries each build once a day.
 */
function attemptedRecently(commit: string): boolean {
  try {
    const stored: unknown = JSON.parse(localStorage.getItem(ATTEMPT_KEY) ?? 'null');
    if (typeof stored !== 'object' || stored === null) return false;
    const { commit: tried, at } = stored as { commit?: unknown; at?: unknown };
    return tried === commit && typeof at === 'number' && Date.now() - at < ATTEMPT_COOLDOWN_MS;
  } catch {
    return false;
  }
}

function rememberAttempt(commit: string) {
  try {
    localStorage.setItem(ATTEMPT_KEY, JSON.stringify({ commit, at: Date.now() }));
  } catch {
    // Without storage the guard is lost, not the update.
  }
}

interface UpdateState {
  update: AppUpdate | null;
  open: boolean;
  phase: UpdatePhase;
  progress: UpdateProgress | null;
  error: AppErrorInfo | null;
  lastCheckedAt: number;
  /** Whether a check has been answered: "up to date" is only said once one has. */
  checked: boolean;
  /** The build the user said "later" to, which is not offered again unasked. */
  postponed: string | null;
  /**
   * Desktop: this build's installer has already been run and the old build is
   * still here, so it is not run again until the user asks on the About page.
   */
  stalled: boolean;
  /**
   * Look for a newer build. An automatic check stays silent about failures --
   * being offline is no reason to interrupt anyone -- and does not re-offer a
   * build the user postponed. A manual one does both.
   */
  check: (manual?: boolean) => Promise<AppUpdate | null>;
  install: () => Promise<void>;
  /** Desktop: hand over to the staged installer. The app exits when it works. */
  apply: () => Promise<void>;
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
  checked: false,
  postponed: null,
  stalled: false,

  check: async (manual = false) => {
    const busy = get().phase;
    if (busy === 'downloading' || busy === 'applying') return get().update;
    set({ lastCheckedAt: Date.now() });

    let found: AppUpdate | null;
    try {
      found = await ipc.checkAppUpdate();
    } catch (caught) {
      if (manual) throw caught;
      return null;
    }

    if (!found) {
      set({ update: null, checked: true, stalled: false, phase: 'idle', error: null, progress: null });
      return null;
    }
    // Only the phone asks. The desktop has no prompt to open: `DesktopUpdater`
    // installs what is found here without a word.
    const offer = IS_MOBILE && (manual || found.commit !== get().postponed);
    set((state) => ({
      update: found,
      checked: true,
      open: state.open || offer,
      // Asking by hand is asking for another attempt.
      stalled: !IS_MOBILE && !manual && attemptedRecently(found.commit),
      // A different build replaces whatever state the previous one was in.
      ...(state.update?.commit !== found.commit
        ? { phase: 'idle' as const, error: null, progress: null }
        : {}),
    }));
    return found;
  },

  install: async () => {
    const { update, phase } = get();
    if (!update || phase === 'downloading' || phase === 'applying') return;
    // A second press on the phone opens the system installer again. On the
    // desktop the file is already where it needs to be.
    if (!IS_MOBILE && phase === 'ready') return;
    set({ phase: 'downloading', progress: null, error: null });
    try {
      await ipc.installAppUpdate(update);
      set({ phase: 'ready' });
    } catch (caught) {
      set({ phase: 'failed', error: ipc.toAppError(caught) });
    }
  },

  apply: async () => {
    const { update, phase } = get();
    if (IS_MOBILE || !update || phase !== 'ready') return;
    // Written first: once the installer is running, nothing after this line
    // can be counted on to.
    rememberAttempt(update.commit);
    set({ phase: 'applying', error: null });
    try {
      await ipc.applyAppUpdate(update);
    } catch (caught) {
      set({ phase: 'failed', error: ipc.toAppError(caught), stalled: true });
    }
  },

  postpone: () => {
    if (get().phase === 'downloading') return;
    set((state) => ({ open: false, postponed: state.update?.commit ?? null }));
  },

  setProgress: (progress) => set({ progress }),
}));

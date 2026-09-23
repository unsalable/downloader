import { create } from 'zustand';

import * as ipc from '@/services/ipc';
import type {
  AppErrorInfo,
  ToolInstallProgress,
  ToolKind,
  ToolsState,
  ToolUpdateCheck,
} from '@/types';

/** The last "check for update" on one tool, kept for the rest of the session. */
export interface ToolCheckState {
  checking: boolean;
  result: ToolUpdateCheck | null;
  error: AppErrorInfo | null;
}

interface ToolsStoreState {
  tools: ToolsState | null;
  /** Discovery runs a subprocess per tool, so there is a real "checking" state. */
  checking: boolean;
  installing: Partial<Record<ToolKind, ToolInstallProgress>>;
  error: string | null;
  checks: Partial<Record<ToolKind, ToolCheckState>>;
  load: () => Promise<void>;
  refresh: () => Promise<void>;
  apply: (tools: ToolsState) => void;
  install: (tool: ToolKind) => Promise<boolean>;
  /**
   * Ask whether a newer release than the copy in use exists. Resolves true
   * only when one does, so the caller installs exactly then. A quiet check
   * shows no spinner and keeps a failure to itself.
   */
  checkUpdate: (tool: ToolKind, options?: { quiet?: boolean }) => Promise<boolean>;
  setInstallProgress: (progress: ToolInstallProgress) => void;
}

export const useToolsStore = create<ToolsStoreState>((set, get) => ({
  tools: null,
  checking: true,
  installing: {},
  error: null,
  checks: {},

  load: async () => {
    // The backend answers once its first discovery pass has finished, so this
    // is a real result rather than a "missing" placeholder.
    try {
      set({ tools: await ipc.getTools(), checking: false });
    } catch {
      set({ checking: false });
    }
  },

  refresh: async () => {
    set({ checking: true });
    try {
      set({ tools: await ipc.refreshTools(), checking: false });
    } catch {
      set({ checking: false });
    }
  },

  apply: (tools) => set({ tools, checking: false }),

  install: async (tool) => {
    if (get().installing[tool]) return false;

    set((state) => ({
      error: null,
      installing: {
        ...state.installing,
        [tool]: { tool, receivedBytes: 0, totalBytes: null, stage: 'downloading' },
      },
    }));

    try {
      const tools = await ipc.installTool(tool);
      set({ tools });
      return true;
    } catch (error) {
      set({ error: ipc.toAppError(error).message });
      // A failed install can still have changed what is on disk, so take the
      // backend's view rather than keeping the one from before.
      try {
        set({ tools: await ipc.getTools() });
      } catch {
        // The tools event carries the same state; nothing more to do here.
      }
      return false;
    } finally {
      set((state) => {
        const next = { ...state.installing };
        delete next[tool];
        return { installing: next };
      });
    }
  },

  checkUpdate: async (tool, { quiet = false } = {}) => {
    const { installing, checks } = get();
    if (installing[tool] || checks[tool]?.checking) return false;

    const record = (next: ToolCheckState) =>
      set((state) => ({ checks: { ...state.checks, [tool]: next } }));

    // The previous answer goes while the new one is asked for, so an answer
    // that comes back the same still reads as having been given again.
    if (!quiet) record({ checking: true, result: null, error: null });
    try {
      const result = await ipc.checkToolUpdate(tool);
      // A press while a quiet check was out is its own question. Answering it
      // from here would end its spinner early and let a second press through.
      if (!(quiet && get().checks[tool]?.checking)) {
        record({ checking: false, result, error: null });
      }
      return !result.upToDate;
    } catch (error) {
      if (!quiet) record({ checking: false, result: null, error: ipc.toAppError(error) });
      return false;
    }
  },

  setInstallProgress: (progress) =>
    set((state) => {
      // Progress can trail the install's own result by a tick; a late event
      // must not bring a finished install's progress bar back.
      if (!state.installing[progress.tool] || progress.stage === 'done') return {};
      return { installing: { ...state.installing, [progress.tool]: progress } };
    }),
}));

/** The engine is what makes analysis possible at all. */
export function selectEngineReady(state: ToolsStoreState): boolean {
  return state.tools?.engine.available ?? false;
}

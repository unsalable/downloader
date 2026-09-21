import { create } from 'zustand';

import * as ipc from '@/services/ipc';
import type { ToolInstallProgress, ToolKind, ToolsState } from '@/types';

interface ToolsStoreState {
  tools: ToolsState | null;
  /** Discovery runs a subprocess per tool, so there is a real "checking" state. */
  checking: boolean;
  installing: Partial<Record<ToolKind, ToolInstallProgress>>;
  error: string | null;
  load: () => Promise<void>;
  refresh: () => Promise<void>;
  apply: (tools: ToolsState) => void;
  install: (tool: ToolKind) => Promise<boolean>;
  setInstallProgress: (progress: ToolInstallProgress) => void;
}

export const useToolsStore = create<ToolsStoreState>((set, get) => ({
  tools: null,
  checking: true,
  installing: {},
  error: null,

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

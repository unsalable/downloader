import { create } from 'zustand';

import * as ipc from '@/services/ipc';
import type { ToolInstallProgress, ToolsState } from '@/types';

type ToolName = 'engine' | 'ffmpeg';

interface ToolsStoreState {
  tools: ToolsState | null;
  /** Discovery runs a subprocess per tool, so there is a real "checking" state. */
  checking: boolean;
  installing: Partial<Record<ToolName, ToolInstallProgress>>;
  error: string | null;
  load: () => Promise<void>;
  refresh: () => Promise<void>;
  apply: (tools: ToolsState) => void;
  install: (tool: ToolName) => Promise<boolean>;
  setInstallProgress: (progress: ToolInstallProgress) => void;
}

export const useToolsStore = create<ToolsStoreState>((set, get) => ({
  tools: null,
  checking: true,
  installing: {},
  error: null,

  load: async () => {
    const tools = await ipc.getTools();
    set({ tools, checking: false });
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
    set((state) => ({
      installing: { ...state.installing, [progress.tool]: progress },
    })),
}));

/** The engine is what makes analysis possible at all. */
export function selectEngineReady(state: ToolsStoreState): boolean {
  return state.tools?.engine.available ?? false;
}

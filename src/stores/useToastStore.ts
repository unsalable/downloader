import { create } from 'zustand';

export type ToastTone = 'info' | 'success' | 'error' | 'warning';

export interface ToastAction {
  label: string;
  onClick: () => void;
  primary?: boolean;
}

export interface Toast {
  id: string;
  tone: ToastTone;
  title: string;
  body?: string;
  actions?: ToastAction[];
  /** 0 keeps the toast until it is dismissed explicitly. */
  durationMs: number;
  /** Replaces any existing toast with the same key instead of stacking. */
  dedupeKey?: string;
}

interface ToastState {
  toasts: Toast[];
  push: (toast: Omit<Toast, 'id' | 'durationMs'> & { durationMs?: number }) => string;
  dismiss: (id: string) => void;
  dismissByKey: (key: string) => void;
  clear: () => void;
}

const DEFAULT_DURATION = 4200;
const MAX_VISIBLE = 4;

let counter = 0;

export const useToastStore = create<ToastState>((set, get) => ({
  toasts: [],

  push: (input) => {
    const id = `toast-${(counter += 1)}`;
    const toast: Toast = {
      id,
      durationMs: input.durationMs ?? DEFAULT_DURATION,
      ...input,
    };

    set((state) => {
      const withoutDuplicate = toast.dedupeKey
        ? state.toasts.filter((existing) => existing.dedupeKey !== toast.dedupeKey)
        : state.toasts;
      // Oldest toasts fall off the top rather than growing an unbounded column.
      const next = [...withoutDuplicate, toast];
      return { toasts: next.slice(Math.max(0, next.length - MAX_VISIBLE)) };
    });

    if (toast.durationMs > 0) {
      window.setTimeout(() => get().dismiss(id), toast.durationMs);
    }
    return id;
  },

  dismiss: (id) =>
    set((state) => ({ toasts: state.toasts.filter((toast) => toast.id !== id) })),

  dismissByKey: (key) =>
    set((state) => ({ toasts: state.toasts.filter((toast) => toast.dedupeKey !== key) })),

  clear: () => set({ toasts: [] }),
}));

/** Imperative helper for modules that are not React components. */
export const toast = {
  info: (title: string, body?: string) =>
    useToastStore.getState().push({ tone: 'info', title, body }),
  success: (title: string, body?: string) =>
    useToastStore.getState().push({ tone: 'success', title, body }),
  error: (title: string, body?: string) =>
    useToastStore.getState().push({ tone: 'error', title, body, durationMs: 7000 }),
  warning: (title: string, body?: string) =>
    useToastStore.getState().push({ tone: 'warning', title, body, durationMs: 6000 }),
};

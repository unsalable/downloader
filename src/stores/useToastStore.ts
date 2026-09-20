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
  /** Hold every countdown while the column is under the pointer or holds focus. */
  pause: () => void;
  resume: () => void;
}

const DEFAULT_DURATION = 4200;
const MAX_VISIBLE = 4;

let counter = 0;

/**
 * Live countdowns, outside the store because they are not rendered.
 *
 * A toast that carries an action -- "Analyze", "Downloads" -- is something the
 * user has to read and then reach for, and the reaching is exactly when the
 * timer used to run out. So the clock is kept here where it can be stopped:
 * how long is left is remembered when the column is paused and the timer is
 * restarted from that point, rather than the toast being pinned open forever
 * or its full duration starting again.
 */
const timers = new Map<string, { handle: number; remaining: number; startedAt: number }>();
let paused = false;

function startTimer(id: string, durationMs: number, onElapsed: (id: string) => void) {
  if (durationMs <= 0) return;
  const handle = window.setTimeout(() => {
    timers.delete(id);
    onElapsed(id);
  }, durationMs);
  timers.set(id, { handle, remaining: durationMs, startedAt: Date.now() });
}

function stopTimer(id: string) {
  const timer = timers.get(id);
  if (!timer) return;
  window.clearTimeout(timer.handle);
  timers.delete(id);
}

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
        ? state.toasts.filter((existing) => {
            const duplicate = existing.dedupeKey === toast.dedupeKey;
            if (duplicate) stopTimer(existing.id);
            return !duplicate;
          })
        : state.toasts;
      // Oldest toasts fall off the top rather than growing an unbounded column.
      const next = [...withoutDuplicate, toast];
      const dropped = next.slice(0, Math.max(0, next.length - MAX_VISIBLE));
      for (const gone of dropped) stopTimer(gone.id);
      return { toasts: next.slice(dropped.length) };
    });

    // A toast that arrives while the column is held open joins the hold, so
    // reading one does not start the next one's clock behind it.
    if (!paused) startTimer(id, toast.durationMs, (expired) => get().dismiss(expired));
    else if (toast.durationMs > 0) {
      timers.set(id, { handle: 0, remaining: toast.durationMs, startedAt: Date.now() });
    }
    return id;
  },

  dismiss: (id) => {
    stopTimer(id);
    set((state) => ({ toasts: state.toasts.filter((toast) => toast.id !== id) }));
  },

  dismissByKey: (key) =>
    set((state) => ({
      toasts: state.toasts.filter((toast) => {
        if (toast.dedupeKey !== key) return true;
        stopTimer(toast.id);
        return false;
      }),
    })),

  clear: () => {
    for (const id of [...timers.keys()]) stopTimer(id);
    set({ toasts: [] });
  },

  pause: () => {
    if (paused) return;
    paused = true;
    for (const [id, timer] of timers) {
      window.clearTimeout(timer.handle);
      timers.set(id, {
        handle: 0,
        remaining: Math.max(0, timer.remaining - (Date.now() - timer.startedAt)),
        startedAt: Date.now(),
      });
    }
  },

  resume: () => {
    if (!paused) return;
    paused = false;
    const dismiss = get().dismiss;
    for (const [id, timer] of [...timers]) {
      timers.delete(id);
      startTimer(id, Math.max(400, timer.remaining), dismiss);
    }
  },
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

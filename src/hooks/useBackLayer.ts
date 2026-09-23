import { useLayoutEffect, useRef } from 'react';

import { IS_MOBILE } from '@/lib/platform';

/*
 * What stands open over a phone screen -- a dialog, a menu -- and so answers
 * the system Back gesture before the screen under it does.
 *
 * Only a list. What Back does with it, and the history entry each one is given
 * so the gesture reaches the app at all, are the shell's (see App): the
 * webview's history is one thing, and two parts of the app writing to it
 * separately is how an entry gets left behind for a Back to land on and do
 * nothing. A layer says it is open and how to close it, and nothing else.
 *
 * Kept in opening order, which is the order they stand in: a menu opened
 * inside a dialog is above it, and goes first.
 */

export interface BackLayer {
  /** Asked to close when Back is pressed. It may stay: a dialog can refuse. */
  close: () => void;
}

const layers: BackLayer[] = [];
const listeners = new Set<() => void>();

function notify() {
  for (const listener of listeners) listener();
}

/** Everything open over the screen, the topmost last. */
export function openLayers(): readonly BackLayer[] {
  return layers;
}

/** Heard whenever a layer opens or closes, from inside the commit that did it. */
export function onLayersChange(listener: () => void): () => void {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}

/**
 * Stand over the screen for as long as `open` is true, on a phone; a desktop
 * has no Back gesture, and nothing is kept there.
 *
 * Registered in a layout effect, so the list is right by the time anything
 * reads it after a render -- including the shell asking, straight after it
 * closed one, whether it really went. The closer is read when Back comes
 * rather than when the layer opened: callers hand a new function on every
 * render, and opening and closing the layer each time would write to the
 * history each time.
 */
export function useBackLayer(open: boolean, close: () => void) {
  const latest = useRef(close);
  useLayoutEffect(() => {
    latest.current = close;
  });

  useLayoutEffect(() => {
    if (!IS_MOBILE || !open) return;
    const layer: BackLayer = { close: () => latest.current() };
    layers.push(layer);
    notify();
    return () => {
      const index = layers.indexOf(layer);
      if (index < 0) return;
      layers.splice(index, 1);
      notify();
    };
  }, [open]);
}

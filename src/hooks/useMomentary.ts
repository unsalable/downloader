import { useCallback, useEffect, useRef, useState } from 'react';

/**
 * True for a moment after `fire()`.
 *
 * Nothing pops up to say that text reached the clipboard or a cache was
 * emptied, so the control that did it answers for itself: its icon turns into a
 * check for as long as this holds, and then turns back.
 */
export function useMomentary(durationMs = 1500): [active: boolean, fire: () => void] {
  const [active, setActive] = useState(false);
  const timer = useRef<number | null>(null);

  useEffect(
    () => () => {
      if (timer.current !== null) window.clearTimeout(timer.current);
    },
    [],
  );

  const fire = useCallback(() => {
    setActive(true);
    // A second press starts the moment again rather than ending it early.
    if (timer.current !== null) window.clearTimeout(timer.current);
    timer.current = window.setTimeout(() => setActive(false), durationMs);
  }, [durationMs]);

  return [active, fire];
}

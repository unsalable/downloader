import { IS_MOBILE } from '@/lib/platform';
import * as ipc from '@/services/ipc';

// Each request waits for the one before it. The command runs on a thread of
// its own, so two sent together -- the film mounted, let go and mounted again
// in one moment, as React does in development -- could reach the screen in
// either order, and the last one asked for must be the one that holds.
let pending: Promise<void> = Promise.resolve();

/**
 * Hold the phone's screen upright, or let it turn with the phone again.
 *
 * For the intro film, which is drawn for a phone held upright; the rest of the
 * app turns with the phone. A desktop has no way up and is not asked. A build
 * without the command turns the call down, and the screen then turns as it
 * always has: nothing the user needs to hear about.
 */
export function setPortraitLock(locked: boolean): Promise<void> {
  if (!IS_MOBILE) return Promise.resolve();
  pending = pending
    .then(() => ipc.platformSetPortraitLock(locked))
    .catch(() => {
      // Turning with the phone is the screen's ordinary behaviour.
    });
  return pending;
}

import { platform } from '@tauri-apps/plugin-os';
import { openPath, revealItemInDir } from '@tauri-apps/plugin-opener';

import * as ipc from '@/services/ipc';

/**
 * Whether this is the phone build. Read once: it cannot change while the app
 * runs, and components branch on it during render.
 *
 * The desktop layout, drag and drop, folder pickers, the tray and keyboard
 * shortcuts all assume a mouse and a file system the user can browse. On a
 * phone those give way to a bottom navigation bar, the system file picker and
 * the share sheet.
 */
export const IS_MOBILE: boolean = (() => {
  try {
    const name = platform();
    return name === 'android' || name === 'ios';
  } catch {
    return false;
  }
})();

/** Open a finished file in the app the OS associates with it. */
export function openFile(path: string): Promise<void> {
  return IS_MOBILE ? ipc.platformOpenFile(path) : openPath(path);
}

/**
 * Show where a file is. A phone has no folder window to select it in, so the
 * system's Downloads view -- which is where it is -- stands in.
 */
export function revealFile(path: string): Promise<void> {
  return IS_MOBILE ? ipc.platformOpenDownloads() : revealItemInDir(path);
}

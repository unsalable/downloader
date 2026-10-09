import { platform } from '@tauri-apps/plugin-os';

/**
 * Whether this is the phone build. Read once: it cannot change while the app
 * runs, and components branch on it during render.
 *
 * The desktop layout, drag and drop, folder pickers, the tray and keyboard
 * shortcuts all assume a mouse and a file system the user can browse. On a
 * phone those give way to a bottom navigation bar, the system file picker and
 * the share sheet.
 *
 * Opening, showing and sharing a finished file do not branch on it: the
 * backend does each the way its own platform does (see `useFileActions`).
 */
export const IS_MOBILE: boolean = (() => {
  try {
    const name = platform();
    return name === 'android' || name === 'ios';
  } catch {
    return false;
  }
})();

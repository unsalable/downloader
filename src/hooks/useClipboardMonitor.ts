import { readText } from '@tauri-apps/plugin-clipboard-manager';
import { useEffect, useRef } from 'react';

import { extractFirstUrl } from '@/lib/url';

/**
 * Watches the clipboard for a media link the user copied elsewhere.
 *
 * Deliberately not a polling loop. To copy a link from a browser, that browser
 * had to have focus -- so this window regaining focus is exactly the moment a
 * new link can have appeared. Checking then costs nothing while the app sits
 * idle, which a timer would not.
 */
export function useClipboardMonitor(
  enabled: boolean,
  onLink: (url: string) => void,
): void {
  const lastSeen = useRef<string | null>(null);
  const handler = useRef(onLink);
  handler.current = onLink;

  useEffect(() => {
    if (!enabled) {
      lastSeen.current = null;
      return;
    }

    let active = true;

    const check = async () => {
      try {
        const text = await readText();
        if (!active || !text) return;

        const url = extractFirstUrl(text);
        // Only prompt once per distinct link: re-focusing the window
        // repeatedly must not re-open the same suggestion.
        if (!url || url === lastSeen.current) return;

        lastSeen.current = url;
        handler.current(url);
      } catch {
        // Clipboard access can fail transiently (another app holding it open).
        // There is nothing useful to tell the user about that.
      }
    };

    const onFocus = () => void check();
    const onVisibility = () => {
      if (document.visibilityState === 'visible') void check();
    };

    window.addEventListener('focus', onFocus);
    document.addEventListener('visibilitychange', onVisibility);

    return () => {
      active = false;
      window.removeEventListener('focus', onFocus);
      document.removeEventListener('visibilitychange', onVisibility);
    };
  }, [enabled]);
}

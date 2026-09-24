import { MotionConfig } from 'motion/react';
import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';

import { App } from './App';
import { IS_MOBILE } from './lib/platform';
import './styles/globals.css';

// The stylesheet trims effects a phone pays too much for; see globals.css.
if (IS_MOBILE) document.documentElement.dataset.mobile = 'true';

// index.html starts dark, the desktop's default. A phone's default follows the
// phone, and its window opens in the phone's theme, so until the saved theme
// arrives the page does the same rather than flashing dark on a light phone.
if (IS_MOBILE && !window.matchMedia('(prefers-color-scheme: dark)').matches) {
  document.documentElement.classList.remove('dark');
  document.documentElement.style.colorScheme = 'light';
}

const container = document.getElementById('root');
if (!container) throw new Error('the root element is missing from index.html');

/**
 * The right-click menu in a webview is a browser affordance (Reload, View
 * Source, Inspect) that has no place in a desktop utility. Text fields keep
 * theirs so copy and paste still work by mouse.
 */
document.addEventListener('contextmenu', (event) => {
  const target = event.target as HTMLElement | null;
  if (target?.closest('input, textarea, [contenteditable="true"], .selectable')) return;
  event.preventDefault();
});

// The webview would otherwise navigate away when a link is dropped onto it,
// replacing the app with the dropped page. Home handles drops itself.
window.addEventListener('dragover', (event) => event.preventDefault());
window.addEventListener('drop', (event) => event.preventDefault());

createRoot(container).render(
  <StrictMode>
    {/* The OS preference, which Motion ignores unless told. The in-app toggle
        is applied globally by the settings store. */}
    <MotionConfig reducedMotion="user">
      <App />
    </MotionConfig>
  </StrictMode>,
);

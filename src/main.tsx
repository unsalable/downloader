import { MotionConfig } from 'motion/react';
import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';

import { App } from './App';
import './styles/globals.css';

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

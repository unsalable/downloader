import { useEffect } from 'react';

import type { HotkeyAction } from '@/types';

/**
 * Window-level shortcuts.
 *
 * These are deliberately not OS-global: registering Ctrl+V globally would
 * intercept paste in every other application on the machine. They only fire
 * while this window has focus.
 */

type Handlers = Partial<Record<HotkeyAction, () => void>>;

/** "Ctrl+Shift+D" -> a comparable signature for a keyboard event. */
function normalizeAccelerator(accelerator: string): string {
  const parts = accelerator
    .split('+')
    .map((part) => part.trim().toLowerCase())
    .filter(Boolean);

  const modifiers = new Set<string>();
  let key = '';

  for (const part of parts) {
    if (part === 'ctrl' || part === 'control' || part === 'cmdorctrl') modifiers.add('ctrl');
    else if (part === 'shift') modifiers.add('shift');
    else if (part === 'alt') modifiers.add('alt');
    else if (part === 'meta' || part === 'cmd' || part === 'super') modifiers.add('meta');
    else key = part;
  }

  return [...['ctrl', 'shift', 'alt', 'meta'].filter((m) => modifiers.has(m)), key].join('+');
}

function eventSignature(event: KeyboardEvent): string {
  const modifiers: string[] = [];
  if (event.ctrlKey) modifiers.push('ctrl');
  if (event.shiftKey) modifiers.push('shift');
  if (event.altKey) modifiers.push('alt');
  if (event.metaKey) modifiers.push('meta');

  let key = event.key.toLowerCase();
  if (key === ' ') key = 'space';
  if (key === 'escape') key = 'esc';

  return [...modifiers, key].join('+');
}

function isTypingTarget(target: EventTarget | null): boolean {
  if (!(target instanceof HTMLElement)) return false;
  return (
    target.tagName === 'INPUT' ||
    target.tagName === 'TEXTAREA' ||
    target.isContentEditable ||
    target.getAttribute('role') === 'combobox'
  );
}

/** Shortcuts that must keep working while the URL field has focus. */
const ALLOWED_WHILE_TYPING: HotkeyAction[] = ['pasteUrl', 'download'];

export function useHotkeys(bindings: Record<HotkeyAction, string>, handlers: Handlers) {
  useEffect(() => {
    const lookup = new Map<string, HotkeyAction>();
    for (const [action, accelerator] of Object.entries(bindings)) {
      if (accelerator) lookup.set(normalizeAccelerator(accelerator), action as HotkeyAction);
    }

    const onKeyDown = (event: KeyboardEvent) => {
      const action = lookup.get(eventSignature(event));
      if (!action) return;

      const handler = handlers[action];
      if (!handler) return;

      // Ctrl+V inside a text field must still paste normally; the app-level
      // handler only takes over outside one.
      if (isTypingTarget(event.target) && !ALLOWED_WHILE_TYPING.includes(action)) return;
      if (action === 'pasteUrl' && isTypingTarget(event.target)) return;

      event.preventDefault();
      handler();
    };

    window.addEventListener('keydown', onKeyDown);
    return () => window.removeEventListener('keydown', onKeyDown);
  }, [bindings, handlers]);
}

/** Human-readable form for the settings list. */
export function formatAccelerator(accelerator: string): string {
  return accelerator
    .split('+')
    .map((part) => {
      const lower = part.trim().toLowerCase();
      if (lower === 'ctrl' || lower === 'control') return 'Ctrl';
      if (lower === 'shift') return 'Shift';
      if (lower === 'alt') return 'Alt';
      if (lower === 'enter') return 'Enter';
      if (lower === ',') return ',';
      return part.trim().length === 1 ? part.trim().toUpperCase() : part.trim();
    })
    .join(' + ');
}

/** Turn a keydown into the accelerator string used for storage. */
export function acceleratorFromEvent(event: KeyboardEvent | React.KeyboardEvent): string | null {
  const key = event.key;
  if (['Control', 'Shift', 'Alt', 'Meta'].includes(key)) return null;

  const parts: string[] = [];
  if (event.ctrlKey) parts.push('Ctrl');
  if (event.shiftKey) parts.push('Shift');
  if (event.altKey) parts.push('Alt');
  if (event.metaKey) parts.push('Meta');

  // A bare letter would swallow ordinary typing, so require a modifier.
  if (parts.length === 0 && key.length === 1) return null;

  parts.push(key.length === 1 ? key.toUpperCase() : key);
  return parts.join('+');
}

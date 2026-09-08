import { RotateCcw } from 'lucide-react';
import { useEffect, useRef, useState } from 'react';

import { IconButton } from '@/components/ui/IconButton';
import { acceleratorFromEvent, formatAccelerator } from '@/hooks/useHotkeys';
import { useTranslation } from '@/i18n';
import { cn } from '@/lib/cn';

interface HotkeyRecorderProps {
  value: string;
  defaultValue: string;
  onChange: (accelerator: string) => void;
  /** Returns the name of the action already using a combination, if any. */
  findConflict: (accelerator: string) => string | null;
}

export function HotkeyRecorder({
  value,
  defaultValue,
  onChange,
  findConflict,
}: HotkeyRecorderProps) {
  const { t } = useTranslation();
  const [recording, setRecording] = useState(false);
  const [conflict, setConflict] = useState<string | null>(null);
  const buttonRef = useRef<HTMLButtonElement>(null);

  useEffect(() => {
    if (!recording) return;

    const onKeyDown = (event: KeyboardEvent) => {
      event.preventDefault();
      event.stopPropagation();

      if (event.key === 'Escape') {
        setRecording(false);
        setConflict(null);
        return;
      }

      const accelerator = acceleratorFromEvent(event);
      // A bare key returns null: binding one would swallow ordinary typing.
      if (!accelerator) return;

      const clash = findConflict(accelerator);
      if (clash) {
        setConflict(clash);
        return;
      }

      onChange(accelerator);
      setRecording(false);
      setConflict(null);
    };

    // Capture phase so the app's own hotkey handler does not fire the action
    // the user is in the middle of rebinding.
    window.addEventListener('keydown', onKeyDown, true);
    return () => window.removeEventListener('keydown', onKeyDown, true);
  }, [recording, findConflict, onChange]);

  return (
    <div className="flex items-center gap-1.5">
      {conflict && (
        <span className="text-[11.5px] text-error">
          {t('settings.hotkeyConflict', { action: conflict })}
        </span>
      )}

      <button
        ref={buttonRef}
        type="button"
        onClick={() => {
          setRecording((value) => !value);
          setConflict(null);
        }}
        onBlur={() => setRecording(false)}
        className={cn(
          'h-8 min-w-[132px] rounded-lg border px-3 font-mono text-[12px] transition-all duration-150',
          recording
            ? 'border-[var(--accent)] bg-accent-soft text-accent ring-2 ring-[var(--accent-ring)]/30'
            : 'border-[var(--border)] bg-surface text-fg-muted hover:border-[var(--border-strong)] hover:text-fg',
        )}
      >
        {recording ? t('settings.hotkeyRecord') : formatAccelerator(value)}
      </button>

      {value !== defaultValue && (
        <IconButton
          icon={<RotateCcw size={13} />}
          label={t('settings.hotkeyReset')}
          size="sm"
          onClick={() => onChange(defaultValue)}
        />
      )}
    </div>
  );
}

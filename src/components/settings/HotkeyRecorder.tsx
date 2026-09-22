import { RotateCcw } from 'lucide-react';
import { useEffect, useRef, useState } from 'react';

import { IconButton } from '@/components/ui/IconButton';
import { InlineNotice } from '@/components/ui/InlineNotice';
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
    <div className="flex flex-col items-end gap-1.5">
      <div className="flex items-center gap-1.5">
        {value !== defaultValue && (
          <IconButton
            icon={<RotateCcw size={14} />}
            label={t('settings.hotkeyReset')}
            size="sm"
            onClick={() => onChange(defaultValue)}
          />
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
            'pressable tabular h-8 min-w-[132px] rounded-[8px] px-3 text-[12.5px] font-medium',
            recording
              ? 'bg-accent-soft text-accent ring-2 ring-inset ring-[var(--accent)]'
              : 'bg-fill text-fg hover:bg-fill-hover',
          )}
        >
          {recording ? t('settings.hotkeyRecord') : formatAccelerator(value)}
        </button>
      </div>

      {conflict && (
        <InlineNotice tone="error">{t('settings.hotkeyConflict', { action: conflict })}</InlineNotice>
      )}
    </div>
  );
}

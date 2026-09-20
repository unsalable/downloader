import type { ReactNode } from 'react';

import { cn } from '@/lib/cn';
import { IS_MOBILE } from '@/lib/platform';
import { Toggle, ToggleTrack } from './Toggle';

interface SettingRowProps {
  title: ReactNode;
  description?: ReactNode;
  control?: ReactNode;
  /** Renders the control on its own line, for wide inputs. */
  stacked?: boolean;
  children?: ReactNode;
  className?: string;
}

export function SettingRow({
  title,
  description,
  control,
  stacked = false,
  children,
  className,
}: SettingRowProps) {
  return (
    <div
      className={cn(
        'flex gap-4 px-4',
        IS_MOBILE ? (stacked ? 'gap-3 py-4' : 'py-4') : 'py-3.5',
        // Wrapping lets a wide control drop under its label on a narrow screen;
        // the label's minimum width is what decides when that happens.
        stacked ? 'flex-col' : 'flex-wrap items-center justify-between',
        className,
      )}
    >
      <div className={cn(stacked ? 'w-full min-w-0' : 'min-w-[min(100%,12rem)] flex-1')}>
        <RowText title={title} description={description} />
      </div>
      {control && <div className={cn('max-w-full shrink-0', stacked && 'w-full')}>{control}</div>}
      {children}
    </div>
  );
}

function RowText({ title, description }: { title: ReactNode; description?: ReactNode }) {
  return (
    <>
      <div className={cn('font-medium text-fg', IS_MOBILE ? 'text-[15px]' : 'text-[13.5px]')}>
        {title}
      </div>
      {description && (
        <p
          className={cn(
            'mt-0.5 leading-relaxed text-fg-muted',
            IS_MOBILE ? 'text-[13px]' : 'text-[12.5px]',
          )}
        >
          {description}
        </p>
      )}
    </>
  );
}

interface ToggleRowProps {
  title: string;
  description?: ReactNode;
  checked: boolean;
  onChange: (checked: boolean) => void;
  disabled?: boolean;
}

/**
 * A setting that is only on or off. On a phone the whole row is the switch:
 * aiming a fingertip at a small toggle at the edge of the screen is exactly
 * the kind of precision a touch screen should not ask for.
 */
export function ToggleRow({ title, description, checked, onChange, disabled = false }: ToggleRowProps) {
  if (!IS_MOBILE) {
    return (
      <SettingRow
        title={title}
        description={description}
        control={<Toggle checked={checked} onChange={onChange} disabled={disabled} label={title} />}
      />
    );
  }

  return (
    <button
      type="button"
      role="switch"
      aria-checked={checked}
      disabled={disabled}
      onClick={() => onChange(!checked)}
      className={cn(
        'flex w-full items-center gap-4 px-4 py-4 text-left transition-colors duration-150 ease-out-quint',
        'active:bg-surface-hover disabled:pointer-events-none disabled:opacity-40',
      )}
    >
      <div className="min-w-0 flex-1">
        <RowText title={title} description={description} />
      </div>
      <ToggleTrack checked={checked} />
    </button>
  );
}

export function SettingGroup({
  title,
  description,
  children,
  className,
}: {
  title?: ReactNode;
  description?: ReactNode;
  children: ReactNode;
  className?: string;
}) {
  return (
    <section className={cn('flex flex-col gap-2.5', className)}>
      {title && (
        <div className="px-1">
          <h3 className="text-[11px] font-semibold uppercase tracking-[0.08em] text-fg-faint">
            {title}
          </h3>
          {description && <p className="mt-1 text-[12.5px] text-fg-muted">{description}</p>}
        </div>
      )}
      <div className="overflow-hidden rounded-[var(--radius-card)] border border-[var(--border)] bg-surface divide-y divide-[var(--border)]">
        {children}
      </div>
    </section>
  );
}

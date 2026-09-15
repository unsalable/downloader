import type { ReactNode } from 'react';

import { cn } from '@/lib/cn';

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
        'flex gap-4 px-4 py-3.5',
        // Wrapping lets a wide control drop under its label on a narrow screen;
        // the label's minimum width is what decides when that happens.
        stacked ? 'flex-col' : 'flex-wrap items-center justify-between',
        className,
      )}
    >
      <div className={cn(stacked ? 'w-full min-w-0' : 'min-w-[min(100%,12rem)] flex-1')}>
        <div className="text-[13.5px] font-medium text-fg">{title}</div>
        {description && (
          <p className="mt-0.5 text-[12.5px] leading-relaxed text-fg-muted">{description}</p>
        )}
      </div>
      {control && <div className={cn('max-w-full shrink-0', stacked && 'w-full')}>{control}</div>}
      {children}
    </div>
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

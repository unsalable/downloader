import type { LucideIcon } from 'lucide-react';
import type { ReactNode } from 'react';

import { cn } from '@/lib/cn';

interface EmptyStateProps {
  /** A lucide icon, passed as the component: `icon={Download}`. */
  icon: LucideIcon;
  title: string;
  body?: string;
  action?: ReactNode;
  className?: string;
}

/**
 * What a screen shows when it has nothing to list: one muted icon, a line
 * saying so, a line saying what to do about it, and the button that does it.
 * It does not animate in -- the screen change that brought it has already moved.
 */
export function EmptyState({ icon: Icon, title, body, action, className }: EmptyStateProps) {
  return (
    <div className={cn('flex flex-col items-center justify-center px-6 py-16 text-center', className)}>
      <Icon size={36} strokeWidth={1.5} aria-hidden="true" className="mb-4 text-fg-faint" />
      <h3 className="text-[15px] font-semibold text-fg">{title}</h3>
      {body && <p className="mt-1 max-w-[300px] text-[13px] leading-relaxed text-fg-muted">{body}</p>}
      {action && <div className="mt-5">{action}</div>}
    </div>
  );
}

import { motion } from 'motion/react';
import type { ReactNode } from 'react';

import { cn } from '@/lib/cn';
import { T, stagger } from '@/lib/motion';

interface EmptyStateProps {
  illustration: ReactNode;
  title: string;
  body?: string;
  action?: ReactNode;
  className?: string;
}

export function EmptyState({ illustration, title, body, action, className }: EmptyStateProps) {
  return (
    <motion.div
      initial={{ opacity: 0, y: 8 }}
      animate={{ opacity: 1, y: 0 }}
      transition={T.entrance}
      className={cn('flex flex-col items-center justify-center px-6 py-16 text-center', className)}
    >
      <div className="mb-5">{illustration}</div>
      <h3 className="text-[15px] font-semibold text-fg">{title}</h3>
      {body && <p className="mt-1.5 max-w-[300px] text-[13px] leading-relaxed text-fg-muted">{body}</p>}
      {action && <div className="mt-5">{action}</div>}
    </motion.div>
  );
}

/**
 * Idle-queue illustration: the app's own aperture, breathing. Driven by CSS
 * keyframes so it does not schedule React work while it loops.
 */
export function QueueIllustration() {
  return (
    <svg width="96" height="96" viewBox="0 0 96 96" fill="none" aria-hidden="true">
      <defs>
        <linearGradient id="empty-queue-grad" x1="0" y1="0" x2="1" y2="1">
          <stop offset="0%" stopColor="var(--accent)" />
          <stop offset="100%" stopColor="var(--accent-hover)" />
        </linearGradient>
      </defs>

      <circle
        cx="48"
        cy="48"
        r="21"
        stroke="url(#empty-queue-grad)"
        strokeOpacity="0.35"
        strokeWidth="1.5"
        style={{ animation: 'ud-pulse-ring 3.2s ease-out infinite', transformOrigin: '48px 48px' }}
      />
      <circle
        cx="48"
        cy="48"
        r="21"
        stroke="url(#empty-queue-grad)"
        strokeOpacity="0.35"
        strokeWidth="1.5"
        style={{
          animation: 'ud-pulse-ring 3.2s ease-out infinite 1.6s',
          transformOrigin: '48px 48px',
        }}
      />

      <circle cx="48" cy="48" r="21" stroke="var(--border-strong)" strokeWidth="1.5" />
      <path
        d="M48 33a15 15 0 0 1 13 7.5"
        stroke="url(#empty-queue-grad)"
        strokeWidth="2.5"
        strokeLinecap="round"
      />
      <circle cx="48" cy="48" r="5" fill="url(#empty-queue-grad)" fillOpacity="0.9" />
    </svg>
  );
}

/** History illustration: a settled stack of finished items. */
export function HistoryIllustration() {
  return (
    <svg width="96" height="96" viewBox="0 0 96 96" fill="none" aria-hidden="true">
      <defs>
        <linearGradient id="empty-history-grad" x1="0" y1="0" x2="1" y2="1">
          <stop offset="0%" stopColor="var(--accent)" />
          <stop offset="100%" stopColor="var(--accent-hover)" />
        </linearGradient>
      </defs>

      {[0, 1, 2].map((index) => (
        <motion.rect
          key={index}
          x={22 + index * 2}
          y={30 + index * 12}
          width={52 - index * 4}
          height="10"
          rx="4"
          fill="var(--surface-active)"
          stroke="var(--border-strong)"
          strokeWidth="1"
          initial={{ opacity: 0, y: -6 }}
          animate={{ opacity: 1 - index * 0.22, y: 0 }}
          transition={{ ...T.entrance, delay: 0.1 + stagger(index, 0.09) }}
        />
      ))}

      <motion.rect
        x="22"
        y="30"
        width="18"
        height="10"
        rx="4"
        fill="url(#empty-history-grad)"
        initial={{ opacity: 0, scaleX: 0.4 }}
        animate={{ opacity: 0.85, scaleX: 1 }}
        style={{ transformOrigin: '22px 35px' }}
        transition={{ ...T.entrance, delay: 0.35 }}
      />
    </svg>
  );
}

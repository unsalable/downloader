import type { CSSProperties, ReactNode } from 'react';

import { cn } from '@/lib/cn';

/*
 * The grouped list every screen is built from: one rounded card, rows inside
 * it, a hairline between the rows.
 *
 *   <ListGroupLabel>Appearance</ListGroupLabel>
 *   <ListGroup>
 *     <SettingRow ... />
 *     <SettingRow ... />
 *   </ListGroup>
 *
 *   <ListGroup inset={104}>      rows that start with a 72px thumbnail:
 *     {tasks.map(...)}           16 padding + 72 thumbnail + 16 gap
 *   </ListGroup>
 *
 *   <ListGroup tone="fill">      a group inside a dialog
 *
 * Each direct child is a row. The hairlines are drawn by the group (see
 * `.list-group` in globals.css), so a row brings no border or divider of its
 * own -- only its padding, `px-4` to line up with the default inset. A row
 * that reacts to the pointer sets `hover:bg-surface-hover`; the group clips it
 * to the rounded corners. Do not nest a group inside a group.
 */

interface ListGroupProps {
  children: ReactNode;
  /** Where the hairlines start, in pixels from the left edge. */
  inset?: number;
  /**
   * `card` (the default) is a card on the page. `fill` is a group set on
   * something that is already a card's colour -- a dialog -- where a white card
   * on white needs its hairline to be seen, and in the dark theme has none
   * (`--card-edge` is transparent there), so the card would vanish and its
   * dividers float. There the group is a quiet fill instead, as grouped rows
   * are on an iOS sheet. Rows in a `fill` group react with `hover:bg-fill`, not
   * `bg-surface-hover`, which is opaque and in the dark theme would be darker
   * than the group at rest.
   *
   * A variant rather than a className, because `cn` does not merge Tailwind
   * classes: a caller's `bg-fill` beside `bg-surface` would win or lose by the
   * stylesheet's order, not by the caller's.
   */
  tone?: 'card' | 'fill';
  className?: string;
}

export function ListGroup({ children, inset = 16, tone = 'card', className }: ListGroupProps) {
  return (
    <div
      style={{ '--list-inset': `${inset}px` } as CSSProperties}
      className={cn(
        'list-group overflow-hidden rounded-[var(--radius-card)]',
        tone === 'fill' ? 'bg-fill' : 'border border-card-edge bg-surface',
        className,
      )}
    >
      {children}
    </div>
  );
}

/**
 * The second line of a row, under its title. It is exactly one line high and
 * wraps: a value that does not fit drops to a second line that is clipped away,
 * so on a narrow phone it disappears whole instead of being cut mid-word. The
 * row adds its own gap and top margin.
 */
export const ROW_LINE =
  'tabular flex h-[18px] flex-wrap overflow-hidden whitespace-nowrap text-[12.5px] leading-[18px] text-fg-muted';

/** The sentence-case label that sits above a group: a heading one level under
 *  the page's title, which is the only thing above it. */
export function ListGroupLabel({ children, className }: { children: ReactNode; className?: string }) {
  return (
    <h2 className={cn('px-4 text-[13px] font-semibold tracking-normal text-fg-muted', className)}>
      {children}
    </h2>
  );
}

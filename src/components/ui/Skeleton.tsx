import { cn } from '@/lib/cn';

export function Skeleton({
  className,
  rounded = 'md',
}: {
  className?: string;
  rounded?: 'sm' | 'md' | 'lg' | 'full';
}) {
  return (
    <div
      aria-hidden="true"
      className={cn(
        'skeleton',
        rounded === 'sm' && 'rounded-[6px]',
        rounded === 'md' && 'rounded-[var(--radius-thumb)]',
        rounded === 'lg' && 'rounded-[var(--radius-card)]',
        rounded === 'full' && 'rounded-full',
        className,
      )}
    />
  );
}

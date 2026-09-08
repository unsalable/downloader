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
        rounded === 'sm' && 'rounded',
        rounded === 'md' && 'rounded-lg',
        rounded === 'lg' && 'rounded-xl',
        rounded === 'full' && 'rounded-full',
        className,
      )}
    />
  );
}

import { useId } from 'react';

import { cn } from '@/lib/cn';
import { IS_MOBILE } from '@/lib/platform';

interface SliderProps {
  value: number;
  min: number;
  max: number;
  step?: number;
  onChange: (value: number) => void;
  label?: string;
  formatValue?: (value: number) => string;
  disabled?: boolean;
  className?: string;
}

/**
 * Native range input, restyled. Keeping the native element means keyboard and
 * screen-reader behaviour come for free; only the track and thumb are painted.
 */
export function Slider({
  value,
  min,
  max,
  step = 1,
  onChange,
  label,
  formatValue,
  disabled = false,
  className,
}: SliderProps) {
  const id = useId();
  const percent = max === min ? 0 : ((value - min) / (max - min)) * 100;

  return (
    <div className={cn('flex flex-col gap-2', className)}>
      {/* The current value is always shown -- a slider whose position means
          nothing is not a control, it is a guess. The inline label is optional
          because the title often lives in the surrounding settings row. */}
      <div className="flex items-baseline justify-between gap-3">
        {label ? (
          <label htmlFor={id} className="text-[13px] font-medium text-fg">
            {label}
          </label>
        ) : (
          <span aria-hidden="true" />
        )}
        <span className={cn('tabular font-medium text-fg', IS_MOBILE ? 'text-[14px]' : 'text-[13px]')}>
          {formatValue ? formatValue(value) : value}
        </span>
      </div>
      <input
        id={id}
        type="range"
        min={min}
        max={max}
        step={step}
        value={value}
        disabled={disabled}
        onChange={(event) => onChange(Number(event.target.value))}
        style={{
          background: `linear-gradient(to right, var(--accent) ${percent}%, var(--surface-active) ${percent}%)`,
        }}
        className={cn(
          'w-full cursor-pointer appearance-none rounded-full outline-none',
          'disabled:pointer-events-none disabled:opacity-40',
          '[&::-webkit-slider-thumb]:appearance-none',
          // A thumb a finger can find without covering the value it sets.
          IS_MOBILE
            ? 'my-2 h-2 [&::-webkit-slider-thumb]:size-6'
            : 'h-1.5 [&::-webkit-slider-thumb]:size-4',
          '[&::-webkit-slider-thumb]:rounded-full [&::-webkit-slider-thumb]:bg-white',
          '[&::-webkit-slider-thumb]:shadow-[0_1px_4px_rgb(0_0_0/0.4)]',
          '[&::-webkit-slider-thumb]:border [&::-webkit-slider-thumb]:border-black/10',
          '[&::-webkit-slider-thumb]:transition-transform [&::-webkit-slider-thumb]:duration-150',
          '[&::-webkit-slider-thumb]:ease-out-quint',
          'hover:[&::-webkit-slider-thumb]:scale-110 active:[&::-webkit-slider-thumb]:scale-95',
        )}
      />
    </div>
  );
}

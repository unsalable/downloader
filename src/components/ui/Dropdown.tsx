import { AnimatePresence, motion } from 'motion/react';
import { Check, ChevronDown } from 'lucide-react';
import {
  useCallback,
  useEffect,
  useId,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
  type ReactNode,
} from 'react';
import { createPortal } from 'react-dom';

import { cn } from '@/lib/cn';

export interface DropdownOption<T extends string> {
  value: T;
  label: string;
  description?: string;
  /** Right-aligned detail, e.g. an estimated file size. */
  meta?: string;
  icon?: ReactNode;
  disabled?: boolean;
  disabledReason?: string;
}

interface DropdownProps<T extends string> {
  value: T;
  options: DropdownOption<T>[];
  onChange: (value: T) => void;
  label?: string;
  placeholder?: string;
  disabled?: boolean;
  className?: string;
  /** Menu width; defaults to matching the trigger. */
  menuWidth?: number;
  align?: 'start' | 'end';
}

const MENU_MAX_HEIGHT = 320;

export function Dropdown<T extends string>({
  value,
  options,
  onChange,
  label,
  placeholder = '--',
  disabled = false,
  className,
  menuWidth,
  align = 'start',
}: DropdownProps<T>) {
  const [open, setOpen] = useState(false);
  const [rect, setRect] = useState<DOMRect | null>(null);
  const [activeIndex, setActiveIndex] = useState(-1);
  const triggerRef = useRef<HTMLButtonElement>(null);
  const menuRef = useRef<HTMLDivElement>(null);
  /** Whether the active option was last moved by the keyboard rather than the
   *  pointer. Only the keyboard is allowed to scroll the list. */
  const keyboardNav = useRef(true);
  const listboxId = useId();

  const selected = useMemo(
    () => options.find((option) => option.value === value) ?? null,
    [options, value],
  );

  const measure = useCallback(() => {
    const node = triggerRef.current;
    if (node) setRect(node.getBoundingClientRect());
  }, []);

  const openMenu = useCallback(() => {
    if (disabled) return;
    measure();
    keyboardNav.current = true;
    setActiveIndex(options.findIndex((option) => option.value === value));
    setOpen(true);
  }, [disabled, measure, options, value]);

  const closeMenu = useCallback(() => {
    setOpen(false);
    setActiveIndex(-1);
    triggerRef.current?.focus();
  }, []);

  const commit = useCallback(
    (option: DropdownOption<T>) => {
      if (option.disabled) return;
      onChange(option.value);
      setOpen(false);
      setActiveIndex(-1);
      triggerRef.current?.focus();
    },
    [onChange],
  );

  // Only listen while the menu is open -- an always-on document listener would
  // fire for every click in the app.
  useEffect(() => {
    if (!open) return;

    const onPointerDown = (event: PointerEvent) => {
      const target = event.target as Node;
      if (menuRef.current?.contains(target) || triggerRef.current?.contains(target)) return;
      setOpen(false);
      setActiveIndex(-1);
    };
    // A long option list scrolls inside the menu, and those scroll events reach
    // this capture-phase listener as well. Closing on them would dismiss the
    // menu the instant the list moved under the cursor -- which is exactly what
    // hovering a partly visible option does. So only movement outside the menu
    // counts, and it re-anchors the menu to the trigger rather than closing it.
    const onViewportChange = (event: Event) => {
      const target = event.target as Node | null;
      if (target && menuRef.current?.contains(target)) return;
      measure();
    };

    document.addEventListener('pointerdown', onPointerDown, true);
    window.addEventListener('resize', onViewportChange);
    // `true` catches scrolls in any ancestor, which a fixed menu would not follow.
    window.addEventListener('scroll', onViewportChange, true);
    return () => {
      document.removeEventListener('pointerdown', onPointerDown, true);
      window.removeEventListener('resize', onViewportChange);
      window.removeEventListener('scroll', onViewportChange, true);
    };
  }, [open, measure]);

  // Keyboard navigation has to bring the active option into view. Hovering must
  // not: scrolling the list under a stationary cursor slides a different option
  // beneath it, and the highlight would run away down the menu.
  useLayoutEffect(() => {
    if (!open || activeIndex < 0 || !keyboardNav.current) return;
    menuRef.current
      ?.querySelector(`[data-index="${activeIndex}"]`)
      ?.scrollIntoView({ block: 'nearest' });
  }, [open, activeIndex]);

  const step = useCallback(
    (direction: 1 | -1) => {
      keyboardNav.current = true;
      setActiveIndex((current) => {
        const total = options.length;
        if (total === 0) return -1;
        let next = current;
        for (let i = 0; i < total; i += 1) {
          next = (next + direction + total) % total;
          if (!options[next]?.disabled) return next;
        }
        return current;
      });
    },
    [options],
  );

  const onTriggerKeyDown = (event: React.KeyboardEvent) => {
    if (event.key === 'ArrowDown' || event.key === 'ArrowUp' || event.key === 'Enter' || event.key === ' ') {
      event.preventDefault();
      if (!open) openMenu();
      else step(event.key === 'ArrowUp' ? -1 : 1);
    } else if (event.key === 'Escape' && open) {
      event.preventDefault();
      closeMenu();
    }
  };

  const onMenuKeyDown = (event: React.KeyboardEvent) => {
    if (event.key === 'ArrowDown') {
      event.preventDefault();
      step(1);
    } else if (event.key === 'ArrowUp') {
      event.preventDefault();
      step(-1);
    } else if (event.key === 'Home') {
      event.preventDefault();
      keyboardNav.current = true;
      setActiveIndex(options.findIndex((option) => !option.disabled));
    } else if (event.key === 'End') {
      event.preventDefault();
      keyboardNav.current = true;
      setActiveIndex(options.length - 1);
    } else if (event.key === 'Enter' || event.key === ' ') {
      event.preventDefault();
      const option = options[activeIndex];
      if (option) commit(option);
    } else if (event.key === 'Escape' || event.key === 'Tab') {
      event.preventDefault();
      closeMenu();
    }
  };

  // Flip above the trigger when there is not enough room below.
  const width = menuWidth ?? rect?.width ?? 200;
  const spaceBelow = rect ? window.innerHeight - rect.bottom - 12 : 0;
  const dropUp = rect != null && spaceBelow < Math.min(MENU_MAX_HEIGHT, options.length * 44 + 16);
  const maxHeight = Math.max(
    140,
    Math.min(MENU_MAX_HEIGHT, dropUp ? (rect?.top ?? 0) - 12 : spaceBelow),
  );

  return (
    <div className={cn('flex flex-col gap-1.5', className)}>
      {label && (
        <span className="text-[11px] font-medium uppercase tracking-[0.07em] text-fg-faint">
          {label}
        </span>
      )}
      <button
        ref={triggerRef}
        type="button"
        role="combobox"
        aria-expanded={open}
        aria-controls={open ? listboxId : undefined}
        aria-haspopup="listbox"
        disabled={disabled}
        onClick={() => (open ? closeMenu() : openMenu())}
        onKeyDown={onTriggerKeyDown}
        className={cn(
          'group flex h-10 w-full items-center gap-2 rounded-[10px] px-3 text-left',
          'border border-[var(--border)] bg-surface transition-all duration-150',
          'hover:border-[var(--border-strong)] hover:bg-surface-hover',
          'disabled:pointer-events-none disabled:opacity-45',
          open && 'border-[var(--accent)] ring-2 ring-[var(--accent-ring)]/30',
        )}
      >
        {selected?.icon && <span className="shrink-0 text-fg-muted">{selected.icon}</span>}
        <span
          className={cn(
            'min-w-0 flex-1 truncate text-[13.5px] font-medium',
            selected ? 'text-fg' : 'text-fg-faint',
          )}
        >
          {selected?.label ?? placeholder}
        </span>
        {selected?.meta && (
          <span className="tabular shrink-0 text-[12px] text-fg-faint">{selected.meta}</span>
        )}
        <ChevronDown
          size={15}
          className={cn(
            'shrink-0 text-fg-faint transition-transform duration-200',
            open && 'rotate-180',
          )}
        />
      </button>

      {createPortal(
        <AnimatePresence>
          {open && rect && (
            <motion.div
              ref={menuRef}
              id={listboxId}
              role="listbox"
              tabIndex={-1}
              onKeyDown={onMenuKeyDown}
              initial={{ opacity: 0, scale: 0.97, y: dropUp ? 6 : -6 }}
              animate={{ opacity: 1, scale: 1, y: 0 }}
              exit={{ opacity: 0, scale: 0.97, y: dropUp ? 4 : -4, transition: { duration: 0.11 } }}
              transition={{ duration: 0.19, ease: [0.22, 1, 0.36, 1] }}
              style={{
                left: align === 'end' ? rect.right - width : rect.left,
                top: dropUp ? undefined : rect.bottom + 6,
                bottom: dropUp ? window.innerHeight - rect.top + 6 : undefined,
                width,
                maxHeight,
                transformOrigin: dropUp ? 'bottom center' : 'top center',
              }}
              className={cn(
                'fixed z-[900] overflow-y-auto overscroll-contain rounded-[var(--radius-panel)] p-1.5',
                'border border-[var(--border-strong)] bg-[var(--surface)] shadow-floating',
                'glass',
              )}
              autoFocus
            >
              {options.map((option, index) => {
                const isSelected = option.value === value;
                return (
                  <div
                    key={option.value}
                    data-index={index}
                    role="option"
                    aria-selected={isSelected}
                    aria-disabled={option.disabled || undefined}
                    title={option.disabled ? option.disabledReason : undefined}
                    onPointerEnter={() => {
                      if (option.disabled) return;
                      keyboardNav.current = false;
                      setActiveIndex(index);
                    }}
                    onClick={() => commit(option)}
                    className={cn(
                      'flex cursor-pointer items-center gap-2.5 rounded-lg px-2.5 py-2 transition-colors duration-100',
                      option.disabled && 'cursor-not-allowed opacity-40',
                      !option.disabled && activeIndex === index && 'bg-surface-hover',
                    )}
                  >
                    {option.icon && <span className="shrink-0 text-fg-muted">{option.icon}</span>}
                    <div className="min-w-0 flex-1">
                      <div
                        className={cn(
                          'truncate text-[13.5px]',
                          isSelected ? 'font-semibold text-fg' : 'font-medium text-fg',
                        )}
                      >
                        {option.label}
                      </div>
                      {option.description && (
                        <div className="truncate text-[11.5px] text-fg-faint">
                          {option.description}
                        </div>
                      )}
                    </div>
                    {option.meta && (
                      <span className="tabular shrink-0 text-[11.5px] text-fg-faint">
                        {option.meta}
                      </span>
                    )}
                    {isSelected && <Check size={14} className="shrink-0 text-accent" />}
                  </div>
                );
              })}
            </motion.div>
          )}
        </AnimatePresence>,
        document.body,
      )}
    </div>
  );
}

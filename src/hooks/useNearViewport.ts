import { useEffect, useState, type RefObject } from 'react';

/**
 * Whether an element has come within `margin` of the part of its scrolling
 * container that is scrolled into view. It latches: once near, a tile keeps
 * its picture, so scrolling back up never fetches it twice. A playlist of two
 * hundred then asks for a dozen thumbnails, not two hundred at once -- each
 * one a request to Rust and a picture held in memory as text.
 *
 * Without an IntersectionObserver to ask, everything counts as near: every
 * picture is fetched at once, as if nothing were watching at all.
 */
export function useNearViewport(
  target: RefObject<Element | null>,
  root: RefObject<Element | null>,
  margin = '200px',
): boolean {
  const [near, setNear] = useState(() => typeof IntersectionObserver === 'undefined');

  useEffect(() => {
    const element = target.current;
    if (near || !element) return;
    const observer = new IntersectionObserver(
      (records) => {
        if (!records.some((record) => record.isIntersecting)) return;
        setNear(true);
        observer.disconnect();
      },
      { root: root.current, rootMargin: margin },
    );
    observer.observe(element);
    return () => observer.disconnect();
  }, [near, target, root, margin]);

  return near;
}

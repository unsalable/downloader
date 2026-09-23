import { memo, useLayoutEffect, useRef } from 'react';

/*
 * Where the playhead is, held outside React.
 *
 * While the video plays, the position changes on every frame. Kept as state on
 * the page it re-rendered the whole editor sixty times a second -- the
 * inspector, the rail of clips, both canvases, every piece on the timeline --
 * for the sake of one line moving and one number counting. On a phone that is
 * the battery going on work nobody sees.
 *
 * Held here instead, the two things that really move with it -- the line
 * across the timeline and the time beside the play button -- subscribe and
 * write their own few pixels and characters, and everything else reads it at
 * the moment it needs it: a split, a step, a zoom about the playhead, a mark
 * landing on it.
 */

export interface Playhead {
  /** Seconds from the start of the file. */
  get(): number;
  set(seconds: number): void;
  /** Called on every change, with nothing: the listener asks `get` if it cares. */
  subscribe(listener: () => void): () => void;
}

export function createPlayhead(): Playhead {
  let seconds = 0;
  const listeners = new Set<() => void>();
  return {
    get: () => seconds,
    set(next) {
      if (next === seconds) return;
      seconds = next;
      for (const listener of listeners) listener();
    },
    subscribe(listener) {
      listeners.add(listener);
      return () => {
        listeners.delete(listener);
      };
    },
  };
}

/**
 * The playhead's position as text, rewritten in place as it moves.
 *
 * React renders the element and nothing inside it; the text is the
 * subscription's to write, so a frame of playback costs one text node rather
 * than a render. Only written when the words change -- at tenths of a second
 * that is a few times a second, not sixty.
 */
export const PlayheadTime = memo(function PlayheadTime({
  playhead,
  format,
  className,
}: {
  playhead: Playhead;
  /** Stable, or the subscription is made again on every render. */
  format: (seconds: number) => string;
  className?: string;
}) {
  const ref = useRef<HTMLSpanElement>(null);

  useLayoutEffect(() => {
    const node = ref.current;
    if (!node) return;
    const write = () => {
      const text = format(playhead.get());
      if (node.textContent !== text) node.textContent = text;
    };
    write();
    return playhead.subscribe(write);
  }, [format, playhead]);

  return <span ref={ref} className={className} />;
});

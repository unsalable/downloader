import type { Transition, Variants } from 'motion/react';

/**
 * The app's motion language, in one place.
 *
 * Before this existed, nineteen different durations and six spring settings
 * were spelled out across the components, so the same kind of change -- a row
 * arriving, a panel opening -- moved at a different speed depending on which
 * file it was written in. Everything below is chosen once and referred to by
 * name, and the CSS half of the app (`globals.css`) carries the same curves so
 * a Tailwind transition and a Motion animation agree.
 *
 * Three tiers, and every animation belongs to exactly one:
 *
 *   micro      a control answering the pointer -- hover, press, a chevron
 *              flipping. Must feel like part of the click, not a reply to it.
 *   component  a thing appearing, leaving or changing inside the page: a
 *              queue row, a dropdown, a disclosure.
 *   spatial    the page itself rearranging: a modal, a screen change, Home
 *              collapsing its headline away.
 *
 * Motion here only ever answers something the user did. Nothing plays on its
 * own when a screen opens -- no staggered arrivals, no first-impression
 * flourish -- and nothing loops except the spinners and bars that say work is
 * still going on (those live in globals.css).
 *
 * Exits are shorter than entrances throughout. What arrives deserves to be
 * watched; what leaves is already decided, and lingering over it is what makes
 * an interface feel slow.
 */

// -- curves -----------------------------------------------------------------

/**
 * Two curves.
 *
 * `out` is the signature: a quint deceleration that arrives quickly and then
 * settles, which is what makes a panel look like it was placed rather than
 * dropped. It is the one mirrored into globals.css as `--ease-out-quint`,
 * because a CSS transition runs both ways from a single curve and so has no
 * use for the second. `in` accelerates away, and only exits use it.
 */
const EASE = {
  out: [0.22, 1, 0.36, 1],
  in: [0.3, 0, 1, 1],
} as const;

// -- durations (seconds; Motion works in seconds, CSS in milliseconds) ------

const DURATION = {
  /** Pointer feedback. Anything slower reads as lag. */
  micro: 0.15,
  /** The default for something arriving or changing in place. */
  component: 0.26,
  /** A whole region rearranging. */
  spatial: 0.34,
} as const;

// -- transitions ------------------------------------------------------------

/**
 * Ready-made transitions. Components name the tier rather than restating a
 * number, so changing how the whole app feels is one edit here.
 */
export const T = {
  micro: { duration: DURATION.micro, ease: EASE.out },
  microOut: { duration: DURATION.micro * 0.8, ease: EASE.in },

  component: { duration: DURATION.component, ease: EASE.out },
  componentOut: { duration: DURATION.component * 0.6, ease: EASE.in },

  spatial: { duration: DURATION.spatial, ease: EASE.out },
  spatialOut: { duration: DURATION.spatial * 0.55, ease: EASE.in },
} as const satisfies Record<string, Transition>;

/**
 * Springs, for objects that should feel like they have mass. Used where a
 * fixed duration would give the movement away as an animation: a knob thrown
 * across a track, a highlight sliding between tabs, a card landing in a stack.
 *
 * Three, with distinct jobs. A fourth would only be a fifth opinion about the
 * same motion.
 */
export const SPRING = {
  /** Small controls that should feel clicky: quick, and at rest without a wobble. */
  snap: { type: 'spring', stiffness: 620, damping: 40, mass: 0.6 },
  /** Shared-element indicators sliding between positions. No overshoot to speak of. */
  glide: { type: 'spring', stiffness: 480, damping: 40, mass: 0.75 },
  /** Something with size arriving and coming to rest. */
  settle: { type: 'spring', stiffness: 420, damping: 34, mass: 0.85 },
} as const satisfies Record<string, Transition>;

// -- variants ---------------------------------------------------------------

/**
 * The standard entrance: a short rise into place. Used by every card, row and
 * panel, which is what makes them read as the same kind of object.
 *
 * `distance` is the only knob -- a queue row travels less than a preview card
 * because it is smaller and there are more of them.
 */
export function rise(distance = 8): Variants {
  return {
    initial: { opacity: 0, y: distance },
    animate: { opacity: 1, y: 0, transition: T.component },
    exit: { opacity: 0, y: -distance * 0.75, transition: T.componentOut },
  };
}

/** The same rise, pre-built at the common distance. */
export const RISE: Variants = rise();

/**
 * A row leaving a list it was removed from. It shrinks slightly rather than
 * sliding away, so the gap it leaves is what the eye follows -- the rows below
 * closing up are the actual message.
 */
export const LIST_ITEM: Variants = {
  initial: { opacity: 0, y: 8 },
  animate: { opacity: 1, y: 0, transition: T.component },
  exit: { opacity: 0, scale: 0.97, transition: T.componentOut },
};

/** Opacity alone. Only for things that have no position to speak of. */
export const FADE: Variants = {
  initial: { opacity: 0 },
  animate: { opacity: 1, transition: T.component },
  exit: { opacity: 0, transition: T.componentOut },
};

/**
 * A disclosure opening downward. Height is a layout property and animating it
 * is not free, but for a section that opens on demand it is the only honest
 * way to show the content pushing the rest of the page down.
 */
export const COLLAPSE: Variants = {
  initial: { opacity: 0, height: 0 },
  animate: { opacity: 1, height: 'auto', transition: T.component },
  exit: { opacity: 0, height: 0, transition: T.componentOut },
};

/**
 * A screen replacing another. Short, and mostly opacity: the navigation itself
 * already said where the user went, so the page only has to not blink.
 */
export const SCREEN: Variants = {
  initial: { opacity: 0, y: 6 },
  animate: { opacity: 1, y: 0, transition: T.spatial },
  exit: { opacity: 0, y: -4, transition: T.spatialOut },
};

/**
 * A dialog. It arrives from slightly below and slightly small, which reads as
 * coming toward the viewer, and leaves back the way it came at half the
 * distance -- the reverse of the entrance, not a different animation.
 */
export const DIALOG: Variants = {
  initial: { opacity: 0, scale: 0.96, y: 12 },
  animate: { opacity: 1, scale: 1, y: 0, transition: T.spatial },
  exit: { opacity: 0, scale: 0.98, y: 6, transition: T.spatialOut },
};

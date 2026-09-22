/**
 * The backdrop of the Home screen: one soft glow of the accent falling from the
 * top of the window, gone well before the URL field so that nothing but the
 * flat page sits behind anything the user reads or types into.
 *
 * It stands still. A single gradient is painted once and costs nothing after
 * that, which is why the phone and the desktop can share it.
 */
export function Background({ active }: { active: boolean }) {
  if (!active) return null;

  return (
    <div
      aria-hidden="true"
      // Never narrower than it is tall, which on a phone means wider than the
      // screen: fitted to a narrow one, the glow becomes a beam with two sides.
      className="pointer-events-none absolute left-1/2 top-0 h-[42%] w-[max(100%,680px)] -translate-x-1/2 opacity-[0.07] dark:opacity-10"
      style={{
        backgroundImage: 'radial-gradient(ellipse 50% 100% at 50% 0%, var(--accent), transparent)',
      }}
    />
  );
}

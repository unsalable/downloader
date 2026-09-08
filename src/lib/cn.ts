type ClassValue = string | number | null | undefined | false | ClassValue[];

/**
 * Minimal class joiner. Deliberately not `tailwind-merge`: components here own
 * their own variants rather than accepting arbitrary overriding classes, so
 * conflict resolution would be dead weight in the bundle.
 */
export function cn(...values: ClassValue[]): string {
  let out = '';
  for (const value of values) {
    if (!value) continue;
    const part = Array.isArray(value) ? cn(...value) : String(value);
    if (!part) continue;
    out = out ? `${out} ${part}` : part;
  }
  return out;
}

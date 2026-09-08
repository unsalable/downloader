/**
 * URL handling on the frontend is limited to cheap "is this even an address"
 * checks, so the input can react instantly while typing. Which provider will
 * actually handle a link is decided in Rust (`providers::detect`) and asked for
 * over IPC -- keeping a second copy of the platform patterns here would be a
 * guaranteed source of drift.
 */

const SCHEME = /^[a-z][a-z0-9+.-]*:\/\//i;

/** Accepts bare hosts by assuming https, which is what paste usually needs. */
export function normalizeUrl(raw: string): string | null {
  const trimmed = raw.trim();
  if (!trimmed) return null;

  const candidate = SCHEME.test(trimmed) ? trimmed : `https://${trimmed}`;

  let parsed: URL;
  try {
    parsed = new URL(candidate);
  } catch {
    return null;
  }

  if (parsed.protocol !== 'http:' && parsed.protocol !== 'https:') return null;
  // A hostname with no dot and no port is almost certainly not an address the
  // user meant to paste (e.g. typing a search term).
  if (!parsed.hostname.includes('.')) return null;
  if (parsed.hostname.startsWith('.') || parsed.hostname.endsWith('.')) return null;

  return parsed.toString();
}

export function isProbablyUrl(raw: string): boolean {
  return normalizeUrl(raw) !== null;
}

/** Host without "www.", for compact display next to a detected platform. */
export function displayHost(raw: string): string | null {
  const normalized = normalizeUrl(raw);
  if (!normalized) return null;
  try {
    return new URL(normalized).hostname.replace(/^www\./, '');
  } catch {
    return null;
  }
}

/**
 * Pulls the first URL out of arbitrary text. Clipboard and drag payloads often
 * carry a title or surrounding prose alongside the link.
 */
export function extractFirstUrl(text: string): string | null {
  const match = text.match(/\b(?:https?:\/\/|www\.)[^\s<>"'`]+/i);
  if (!match) return null;
  return normalizeUrl(match[0].replace(/[),.;!]+$/, ''));
}

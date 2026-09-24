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
 * A link as a row shows it: without the scheme, the "www." or a trailing
 * slash, which every link has and none is told apart by. The rest -- the path
 * and the query, where a video's id lives -- is kept, and the row cuts off
 * what does not fit.
 *
 * Read the way it was written rather than the way it travels: a stored link
 * has been through `normalizeUrl`, so a Turkish title in a Reddit address
 * arrives as `%C3%BC` escapes, which nobody reads. The clipboard still gets the
 * link exactly as stored. Anything that does not parse is shown as given.
 */
export function displayLink(raw: string): string {
  const trimmed = raw.trim();
  let parsed: URL;
  try {
    parsed = new URL(trimmed);
  } catch {
    return trimmed;
  }

  const host = parsed.host.replace(/^www\./, '');
  const path = parsed.pathname.replace(/\/$/, '');
  const rest = `${path}${parsed.search}`;
  try {
    return `${host}${decodeURI(rest)}`;
  } catch {
    // A lone "%" or a cut-off sequence: shown escaped rather than not at all.
    return `${host}${rest}`;
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

import { useEffect, useState } from 'react';

import * as ipc from '@/services/ipc';

/**
 * Thumbnails are fetched and cached by Rust, then handed over as a data URL.
 * Two reasons: the content-security policy allows no remote image origins, and
 * a cached thumbnail means re-opening History contacts nobody.
 *
 * The in-memory map avoids a round trip for an image already resolved this
 * session -- History scrolling would otherwise re-request the same rows.
 */
const memory = new Map<string, string>();
const inflight = new Map<string, Promise<string>>();

/** Bounded so a long History session cannot grow this without limit. */
const MAX_ENTRIES = 240;

function remember(url: string, dataUrl: string) {
  if (memory.size >= MAX_ENTRIES) {
    const oldest = memory.keys().next().value;
    if (oldest !== undefined) memory.delete(oldest);
  }
  memory.set(url, dataUrl);
}

export function useThumbnail(url: string | null | undefined): {
  src: string | null;
  loading: boolean;
  failed: boolean;
} {
  const [src, setSrc] = useState<string | null>(() => (url ? memory.get(url) ?? null : null));
  const [loading, setLoading] = useState(false);
  const [failed, setFailed] = useState(false);

  useEffect(() => {
    if (!url) {
      setSrc(null);
      setFailed(false);
      return;
    }

    const cached = memory.get(url);
    if (cached) {
      setSrc(cached);
      setFailed(false);
      return;
    }

    let active = true;
    setLoading(true);
    setFailed(false);

    // Share one request per URL: a grid can mount several cards for the same
    // thumbnail in the same frame.
    let request = inflight.get(url);
    if (!request) {
      request = ipc.getThumbnail(url);
      inflight.set(url, request);
      request.finally(() => inflight.delete(url));
    }

    request
      .then((dataUrl) => {
        remember(url, dataUrl);
        if (active) {
          setSrc(dataUrl);
          setLoading(false);
        }
      })
      .catch(() => {
        if (active) {
          setFailed(true);
          setLoading(false);
        }
      });

    return () => {
      active = false;
    };
  }, [url]);

  return { src, loading, failed };
}

export function clearThumbnailMemory() {
  memory.clear();
}

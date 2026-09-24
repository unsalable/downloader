import { describe, expect, it } from 'vitest';

import { displayLink, normalizeUrl } from './url';

describe('displayLink', () => {
  it('drops the scheme, the www and a trailing slash', () => {
    expect(displayLink('https://www.instagram.com/reel/C8kLm2NoPqR/')).toBe(
      'instagram.com/reel/C8kLm2NoPqR',
    );
    expect(displayLink('https://x.com/')).toBe('x.com');
  });

  it('keeps the query, where a video id can live', () => {
    expect(displayLink('https://www.youtube.com/watch?v=dQw4w9WgXcQ&t=10s')).toBe(
      'youtube.com/watch?v=dQw4w9WgXcQ&t=10s',
    );
  });

  it('shows a stored link in its own letters', () => {
    const stored = normalizeUrl('https://www.reddit.com/r/Turkey/comments/1abc/bugün_hava_çok_güzel/')!;
    expect(stored).toContain('%C3%BC');
    expect(displayLink(stored)).toBe('reddit.com/r/Turkey/comments/1abc/bugün_hava_çok_güzel');
  });

  it('leaves what it cannot read as it was', () => {
    expect(displayLink('https://example.com/100%')).toBe('example.com/100%');
    expect(displayLink('  not a link ')).toBe('not a link');
  });
});

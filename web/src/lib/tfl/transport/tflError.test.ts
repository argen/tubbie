/**
 * Ports the redaction contract from `crates/tfl-client/src/error.rs` tests:
 * a `TflError`'s message must never carry an `app_key` or a query string.
 */

import { describe, expect, it } from 'vitest';
import { TflError, sanitizeUrl, truncateTo512 } from './tflError.js';

describe('TflError factories', () => {
  it('notFound carries the kind and a path-only detail', () => {
    const e = TflError.notFound('/StopPoint/TEST/Arrivals');
    expect(e).toBeInstanceOf(TflError);
    expect(e.kind).toBe('NotFound');
    expect(e.message).toContain('StopPoint');
  });

  it('rateLimited carries the retry-after and renders "unknown" when null', () => {
    expect(TflError.rateLimited(6000).retryAfterMs).toBe(6000);
    expect(TflError.rateLimited(null).retryAfterMs).toBeNull();
    expect(TflError.rateLimited(null).message).toContain('unknown');
  });

  it('http carries the status code', () => {
    const e = TflError.http(503, 'service unavailable');
    expect(e.kind).toBe('Http');
    expect(e.status).toBe(503);
    expect(e.message).toContain('503');
  });

  it('transport renders the sanitized URL but no query string', () => {
    const e = TflError.transport('timeout', 'https://api.tfl.gov.uk/StopPoint/TEST/Arrivals');
    expect(e.kind).toBe('Transport');
    expect(e.message).toContain('StopPoint');
    expect(e.message).not.toContain('?');
    expect(e.message).not.toContain('app_key');
  });
});

describe('sanitizeUrl', () => {
  it('strips the query string (where app_key lives) and the fragment', () => {
    const out = sanitizeUrl('https://api.tfl.gov.uk/StopPoint/X/Arrivals?app_key=DEADBEEF#frag');
    expect(out).toBe('https://api.tfl.gov.uk/StopPoint/X/Arrivals');
    expect(out).not.toContain('DEADBEEF');
    expect(out).not.toContain('app_key');
    expect(out).not.toContain('?');
  });

  it('preserves a non-default port', () => {
    expect(sanitizeUrl('http://127.0.0.1:8080/pool-keys.json?app_key=x')).toBe(
      'http://127.0.0.1:8080/pool-keys.json',
    );
  });

  it('returns a placeholder for an unparseable URL', () => {
    expect(sanitizeUrl('not a url')).toBe('(no url)');
  });
});

describe('truncateTo512', () => {
  it('leaves short strings untouched', () => {
    expect(truncateTo512('hello')).toBe('hello');
  });

  it('truncates long strings and appends an ellipsis', () => {
    const out = truncateTo512('a'.repeat(600));
    expect(out.length).toBeLessThanOrEqual(513);
    expect(out.endsWith('…')).toBe(true);
  });
});

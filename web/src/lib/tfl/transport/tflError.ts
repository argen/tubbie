/**
 * Typed transport errors — port of `tfl_client::error::TflError`.
 *
 * Modelled as a single `Error` subclass carrying a `kind` discriminant rather
 * than a bare union, so callers can both `throw`/`catch` it and `switch` on
 * `err.kind`. The Rust enum is `NotFound | RateLimited | Http | Transport |
 * Parse` (the `ParseAt` / `Io` / `InvalidRequest` variants are Rust-only
 * concerns — fixture file IO and the path-traversal validator — that have no
 * analogue in the browser transport).
 *
 * SECURITY (mirrors the Rust contract): a `TflError` message MUST NEVER contain
 * the request URL's query string — that is where `app_key` lives. Construct
 * `Transport` errors only via {@link TflError.transport} with an already
 * {@link sanitizeUrl}-stripped URL, and never interpolate a raw URL into any
 * other variant's message.
 */

export type TflErrorKind = 'NotFound' | 'RateLimited' | 'Http' | 'Transport' | 'Parse';

interface TflErrorData {
  /** HTTP status — present only for `Http`. */
  status?: number;
  /**
   * Retry-After in milliseconds — present only for `RateLimited`. `null` when
   * the server sent no parseable `Retry-After` header (mirrors Rust
   * `Option<Duration>`).
   */
  retryAfterMs?: number | null;
}

export class TflError extends Error {
  readonly kind: TflErrorKind;
  readonly status: number | undefined;
  readonly retryAfterMs: number | null | undefined;

  private constructor(kind: TflErrorKind, message: string, data: TflErrorData) {
    super(message);
    this.name = 'TflError';
    this.kind = kind;
    this.status = data.status;
    this.retryAfterMs = data.retryAfterMs;
  }

  /** A 404 from TfL (or a genuinely-absent resource). */
  static notFound(detail: string): TflError {
    return new TflError('NotFound', `not found: ${detail}`, {});
  }

  /** A 429. `retryAfterMs` is `null` when no usable `Retry-After` was sent. */
  static rateLimited(retryAfterMs: number | null): TflError {
    const human = retryAfterMs === null ? 'unknown' : `${String(retryAfterMs)}ms`;
    return new TflError('RateLimited', `rate limited by TfL API (retry after: ${human})`, {
      retryAfterMs,
    });
  }

  /** A non-2xx/404/429 HTTP status. `bodySnippet` is already ≤512 chars. */
  static http(status: number, bodySnippet: string): TflError {
    return new TflError('Http', `HTTP ${String(status)} from TfL API: ${bodySnippet}`, { status });
  }

  /**
   * A transport-level failure (network error, timeout, decode error).
   * `urlSanitized` MUST already be stripped of its query string — pass the
   * output of {@link sanitizeUrl}, never a raw request URL.
   */
  static transport(detail: string, urlSanitized: string): TflError {
    return new TflError('Transport', `transport error: ${detail} (url: ${urlSanitized})`, {});
  }

  /** A response body that could not be parsed as JSON. */
  static parse(detail: string): TflError {
    return new TflError('Parse', `parse error: ${detail}`, {});
  }
}

/**
 * Strip the query string and fragment from a URL, leaving
 * `scheme://host[:port]/path`. Mirrors Rust `TflError::transport_from`'s URL
 * sanitisation so `app_key` (a query parameter) can never reach an error
 * message or log line. Returns `(no url)` for an unparseable input.
 */
export function sanitizeUrl(raw: string): string {
  try {
    const u = new URL(raw);
    return `${u.protocol}//${u.host}${u.pathname}`;
  } catch {
    return '(no url)';
  }
}

/**
 * Truncate a body snippet to at most 512 characters for error context,
 * appending an ellipsis when cut. Mirrors Rust `truncate_to_512` (the Rust
 * version counts bytes; here we count UTF-16 code units — close enough for an
 * error snippet, and never used for anything load-bearing).
 */
export function truncateTo512(s: string): string {
  return s.length <= 512 ? s : `${s.slice(0, 512)}…`;
}

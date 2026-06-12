/**
 * `RecordHttp` — an in-memory {@link TflHttp} for tests.
 *
 * Maps `endpoint/id` to a static JSON value or a per-call handler (so retry /
 * partial-warm paths can be driven deterministically: "fail twice, then
 * succeed", "always 429", …). Tracks call counts so a test can assert how many
 * times the wire was hit. An unregistered key resolves to `TflError.notFound`,
 * matching how the live transport and the disk fixtures report a missing
 * resource.
 *
 * Test support only — not part of the production bundle.
 */

import { TflError } from '../transport/tflError.js';
import type { TflHttp } from '../transport/tflHttp.js';

function key(endpoint: string, id: string): string {
  return `${endpoint}/${id}`;
}

/** Resolves the JSON value for a call, or rejects with a {@link TflError}. */
export type Responder = (callNumber: number) => Promise<unknown>;

export class RecordHttp implements TflHttp {
  private readonly handlers = new Map<string, Responder>();
  private readonly counts = new Map<string, number>();

  /** Serve a static JSON value for `endpoint/id`. */
  put(endpoint: string, id: string, value: unknown): this {
    this.handlers.set(key(endpoint, id), () => Promise.resolve(value));
    return this;
  }

  /** Serve via a handler receiving the 1-based call number (for retry tests). */
  putHandler(endpoint: string, id: string, responder: Responder): this {
    this.handlers.set(key(endpoint, id), responder);
    return this;
  }

  fetch(endpoint: string, id: string): Promise<unknown> {
    const k = key(endpoint, id);
    const n = (this.counts.get(k) ?? 0) + 1;
    this.counts.set(k, n);
    const handler = this.handlers.get(k);
    if (handler === undefined) {
      return Promise.reject(TflError.notFound(`no fixture for ${k}`));
    }
    return handler(n);
  }

  /** How many times `endpoint/id` was fetched. */
  callCount(endpoint: string, id: string): number {
    return this.counts.get(key(endpoint, id)) ?? 0;
  }
}

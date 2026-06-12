/**
 * Test-only loader for the repo-root `fixtures/` directory — the same JSON
 * files the Rust `tfl-*` crate tests consume via `FixtureTflHttp`
 * (`{category}/{id}.json`). Reusing one fixture set keeps the ported TypeScript
 * core and the Rust crates asserting against identical wire data, which is the
 * primary drift guard between desktop (TS) and iOS (Rust).
 *
 * Node-only: uses `node:fs`, so import it from vitest, never from the browser
 * bundle. Lives outside `domain/`, `transport/`, etc. for exactly that reason.
 */
import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { dirname, resolve } from 'node:path';

/** Repo root, resolved from this file: web/src/lib/tfl → up 4. */
const REPO_ROOT = resolve(dirname(fileURLToPath(import.meta.url)), '../../../..');
const FIXTURES_DIR = resolve(REPO_ROOT, 'fixtures');

/** The four fixture categories that mirror TfL's endpoint families. */
export type FixtureCategory = 'arrivals' | 'stop-points' | 'line-status' | 'stop-point';

/**
 * Read and JSON-parse `fixtures/{category}/{id}.json`. Returns `unknown` —
 * callers narrow via the domain parsers (`parseStation`, `parseArrival`, …)
 * exactly as the Rust side deserializes the same bytes.
 */
export function loadFixture(category: FixtureCategory, id: string): unknown {
  const path = resolve(FIXTURES_DIR, category, `${id}.json`);
  const raw = readFileSync(path, 'utf-8');
  const data: unknown = JSON.parse(raw);
  return data;
}

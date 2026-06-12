/**
 * Defensive readers for TfL's raw wire JSON (`unknown` after `JSON.parse`).
 *
 * The Rust side parses TfL's camelCase payloads through serde structs with
 * `#[serde(default)]` on most fields. These helpers reproduce that
 * "missing/wrong-typed field → safe default" behaviour without `any` or type
 * assertions, so the domain parsers can narrow `unknown` cleanly under the
 * repo's strict-type-checked ESLint config.
 */

/** True for a non-null, non-array object. Mirrors `ipc/types.ts`'s `isRecord`. */
export function isRecord(v: unknown): v is Record<string, unknown> {
  return typeof v === 'object' && v !== null && !Array.isArray(v);
}

/** Read a string field, or `""` when absent/wrong-typed (serde `default`). */
export function rString(rec: Record<string, unknown>, key: string): string {
  const v = rec[key];
  return typeof v === 'string' ? v : '';
}

/** Read a number field, or `0` when absent/wrong-typed. */
export function rNumber(rec: Record<string, unknown>, key: string): number {
  const v = rec[key];
  return typeof v === 'number' ? v : 0;
}

/** Read an array field, or `[]` when absent/wrong-typed. */
export function rArray(rec: Record<string, unknown>, key: string): unknown[] {
  const v = rec[key];
  return Array.isArray(v) ? v : [];
}

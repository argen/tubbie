/**
 * Severity-code → render-tier bucket mapping — ported from
 * `crates/tfl-domain/src/types.rs` (`severity_bucket`,
 * `SeverityBucket::sort_rank`).
 *
 * This is the **single canonical** mapping for the TypeScript side (invariant
 * #25). `utils/status.ts` consumes `severityBucketSortRank` from here rather
 * than keeping its own rank table, so the worst-first ordering lives in one
 * place. The wire-format seam (`tflLineToLineStatus`, which populates the
 * `bucket` field on each `StatusEntry`) lands with the line-status wire types
 * in the cache layer (Phase 3), where its consumer `getLineStatus` lives.
 */
import type { SeverityBucket } from '$lib/ipc/types.js';

/**
 * Map a TfL numeric severity code (0–20, per
 * https://api.tfl.gov.uk/StatusSeverity) to its render tier. Out-of-range codes
 * fall through to `Other` so a future TfL extension never mis-categorises into
 * a more severe tier or throws.
 */
export function severityBucket(severity: number): SeverityBucket {
  switch (severity) {
    case 1:
    case 2:
    case 16:
    case 20:
      return 'Closed';
    case 3:
    case 4:
    case 5:
    case 11:
      return 'PartClosure';
    case 6:
      return 'SevereDelays';
    case 7:
    case 8:
    case 15:
      return 'ReducedService';
    case 9:
    case 14:
      return 'MinorDelays';
    case 10:
    case 18:
      return 'GoodService';
    case 17:
    case 19:
      return 'Information';
    default:
      return 'Other';
  }
}

/** Worst-first rank — lower = worse, `GoodService` sorts last. */
const SORT_RANK: Record<SeverityBucket, number> = {
  Closed: 0,
  PartClosure: 1,
  SevereDelays: 2,
  ReducedService: 3,
  MinorDelays: 4,
  Information: 5,
  Other: 6,
  GoodService: 7,
};

/**
 * Numeric worst-first ordering key for a bucket. Drives the Status tab's
 * worst-first sort; `GoodService` sorts strictly last so the "all other lines:
 * Good Service" footer grouping is stable.
 */
export function severityBucketSortRank(bucket: SeverityBucket): number {
  return SORT_RANK[bucket];
}

/**
 * Service-status helpers — worst-first ordering and disruption state.
 *
 * Consumes the canonical `bucket` field on each `StatusEntry` (mirrors
 * `tfl_domain::SeverityBucket`, computed at the Rust wire seam — invariant
 * #25). UI code MUST NOT re-map raw `severity` codes; the day TfL adds a code
 * the mapping changes in ONE place (Rust), not here.
 *
 * The only thing duplicated from Rust is the worst-first ORDER of the eight
 * fixed buckets (mirrors `SeverityBucket::sort_rank`). That set is closed —
 * adding a bucket is a deliberate cross-cutting change anyway.
 */
import type { LineStatus, RouteSegment, SeverityBucket, StatusEntry } from '$lib/ipc/types.js';

/** Worst-first rank — lower = worse. Mirrors `SeverityBucket::sort_rank`. */
const BUCKET_RANK: Record<SeverityBucket, number> = {
  Closed: 0,
  PartClosure: 1,
  SevereDelays: 2,
  ReducedService: 3,
  MinorDelays: 4,
  Information: 5,
  Other: 6,
  GoodService: 7,
};

const GOOD_RANK = BUCKET_RANK.GoodService;

/** Numeric worst-first rank for a bucket; unknown/missing → `Other`. */
export function bucketRank(bucket: SeverityBucket | undefined): number {
  return bucket !== undefined ? BUCKET_RANK[bucket] : BUCKET_RANK.Other;
}

/**
 * The worst (lowest-rank) bucket among a line's status entries. A line with no
 * entries is treated as `GoodService` — TfL only omits status when nothing is
 * wrong.
 */
export function worstBucket(line: LineStatus): SeverityBucket {
  let worst: SeverityBucket = 'GoodService';
  for (const entry of line.status) {
    if (bucketRank(entry.bucket) < bucketRank(worst)) {
      worst = entry.bucket ?? 'Other';
    }
  }
  return worst;
}

/** True when a line is anything other than good service. */
export function isDisrupted(line: LineStatus): boolean {
  return bucketRank(worstBucket(line)) < GOOD_RANK;
}

/**
 * Lines sorted worst-first (most severe disruption at the top), ties broken by
 * `line_id` for stable rendering. Does not mutate the input.
 */
export function sortLinesWorstFirst(statuses: LineStatus[]): LineStatus[] {
  return [...statuses].sort((a, b) => {
    const delta = bucketRank(worstBucket(a)) - bucketRank(worstBucket(b));
    return delta !== 0 ? delta : a.line_id.localeCompare(b.line_id);
  });
}

/** True when every line in scope is good service (the calm empty state). */
export function allGoodService(statuses: LineStatus[]): boolean {
  return statuses.every((line) => !isDisrupted(line));
}

/**
 * True when any line within the user's selection is disrupted. `selectedLineIds`
 * is the chip filter (`BoardConfig.line_ids`): empty = all lines (same mask as
 * the board UI and the menu-bar title). Drives the menu-bar disruption icon so
 * a line you filtered out can't flip the indicator.
 */
export function anyDisrupted(statuses: LineStatus[], selectedLineIds: string[]): boolean {
  const active = selectedLineIds.length > 0;
  return statuses.some(
    (line) => (!active || selectedLineIds.includes(line.line_id)) && isDisrupted(line),
  );
}

/** The disrupted lines only, worst-first. */
export function disruptedLinesWorstFirst(statuses: LineStatus[]): LineStatus[] {
  return sortLinesWorstFirst(statuses.filter(isDisrupted));
}

const BUCKET_LABEL: Record<SeverityBucket, string> = {
  Closed: 'Closed',
  PartClosure: 'Part closure',
  SevereDelays: 'Severe delays',
  ReducedService: 'Reduced service',
  MinorDelays: 'Minor delays',
  Information: 'Information',
  Other: 'Service notice',
  GoodService: 'Good service',
};

/** Human-readable label for a bucket (presentation text, not a code remap). */
export function bucketLabel(bucket: SeverityBucket | undefined): string {
  return bucket !== undefined ? BUCKET_LABEL[bucket] : BUCKET_LABEL.Other;
}

/**
 * The short status line for a disrupted line: TfL's own disruption text when
 * present, else the bucket label. Callers should only pass disrupted lines.
 */
export function lineStatusLabel(line: LineStatus): string {
  const text = line.disruption_text?.trim();
  return text && text.length > 0 ? text : bucketLabel(worstBucket(line));
}

/**
 * Safe accessor for `affected_segments` on a `StatusEntry`. Returns the
 * array when present, or `[]` for older payloads that predate the field.
 * Empty array → render "Entire line" in the UI.
 */
export function segmentsFor(entry: StatusEntry): RouteSegment[] {
  return entry.affected_segments ?? [];
}

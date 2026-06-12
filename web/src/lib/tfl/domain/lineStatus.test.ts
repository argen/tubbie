/**
 * Wire-format seam tests — `tflLineToLineStatus` / `buildAffectedSegments`,
 * ported from the `tfl_line_to_line_status` / `build_affected_segments` coverage
 * in `crates/tfl-cache`. Pins the canonical `bucket` (#25), the deduped
 * disruption text, and the unordered affected-segment dedupe.
 */

import { describe, expect, it } from 'vitest';
import { buildAffectedSegments, tflLineToLineStatus } from './status.js';

describe('tflLineToLineStatus', () => {
  it('maps a good-service line to GoodService with no disruption text', () => {
    const ls = tflLineToLineStatus({
      id: 'victoria',
      lineStatuses: [{ statusSeverity: 10, statusSeverityDescription: 'Good Service' }],
    });
    expect(ls.line_id).toBe('victoria');
    expect(ls.status[0]?.bucket).toBe('GoodService');
    expect(ls.disruption_text).toBeNull();
    expect(ls.status[0]?.affected_segments).toEqual([]);
  });

  it('populates the bucket and disruption text from a disrupted line', () => {
    const ls = tflLineToLineStatus({
      id: 'central',
      lineStatuses: [
        {
          statusSeverity: 6,
          statusSeverityDescription: 'Severe Delays',
          reason: 'CENTRAL LINE: Severe delays due to a signal failure.',
          disruption: {
            affectedRoutes: [{ originationName: 'Ealing', destinationName: 'Epping' }],
          },
        },
      ],
    });
    expect(ls.status[0]?.bucket).toBe('SevereDelays');
    expect(ls.disruption_text).toBe('CENTRAL LINE: Severe delays due to a signal failure.');
    expect(ls.status[0]?.affected_segments).toEqual([{ from: 'Ealing', to: 'Epping' }]);
  });

  it('dedupes repeated reason strings, joining the rest with " | "', () => {
    const ls = tflLineToLineStatus({
      id: 'district',
      lineStatuses: [
        { statusSeverity: 9, statusSeverityDescription: 'Minor Delays', reason: 'Earlier fault' },
        { statusSeverity: 9, statusSeverityDescription: 'Minor Delays', reason: 'Earlier fault' },
        { statusSeverity: 9, statusSeverityDescription: 'Minor Delays', reason: 'Signal check' },
      ],
    });
    expect(ls.disruption_text).toBe('Earlier fault | Signal check');
  });

  it('defaults safely on malformed input', () => {
    const ls = tflLineToLineStatus({});
    expect(ls.line_id).toBe('');
    expect(ls.status).toEqual([]);
    expect(ls.disruption_text).toBeNull();
  });
});

describe('buildAffectedSegments', () => {
  it('collapses reverse-duplicate routes to one segment, first-seen order', () => {
    const segs = buildAffectedSegments({
      affectedRoutes: [
        { originationName: 'Watford', destinationName: 'Aldgate' },
        { originationName: 'Aldgate', destinationName: 'Watford' }, // reverse dup
        { originationName: 'Baker Street', destinationName: 'Uxbridge' },
      ],
    });
    expect(segs).toEqual([
      { from: 'Watford', to: 'Aldgate' },
      { from: 'Baker Street', to: 'Uxbridge' },
    ]);
  });

  it('skips routes missing an origin or destination', () => {
    const segs = buildAffectedSegments({
      affectedRoutes: [
        { originationName: '', destinationName: 'Epping' },
        { originationName: 'Ealing', destinationName: '   ' },
        { originationName: 'A', destinationName: 'B' },
      ],
    });
    expect(segs).toEqual([{ from: 'A', to: 'B' }]);
  });

  it('returns [] when there is no disruption object', () => {
    expect(buildAffectedSegments(undefined)).toEqual([]);
    expect(buildAffectedSegments({})).toEqual([]);
  });
});

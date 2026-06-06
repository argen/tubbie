// @vitest-environment happy-dom
/**
 * DOM tests for StatusView — the full TfL-style service-status view.
 *
 * Covers:
 *   - Disrupted line rows: left stripe, line name, per-entry severity sub-headline
 *   - Route segments: "A ↔ B" rows; "Entire line" when empty segments
 *   - Disclosure chevron: toggles aria-expanded and reveals disruption_text
 *   - Footer "Good service on all other lines" (replaces chip enumeration)
 *   - Empty state "Service status unavailable."
 *   - Headline count / freshness / refresh button
 */
import { describe, expect, it, vi } from 'vitest';
import { render, screen, fireEvent } from '@testing-library/svelte';
import type { LineStatus, SeverityBucket, RouteSegment } from '$lib/ipc/types.js';
import StatusView from '../../components/StatusView.svelte';

function makeEntry(bucket: SeverityBucket, description: string, segments: RouteSegment[] = []) {
  return { severity: 0, description, bucket, affected_segments: segments };
}

function line(
  line_id: string,
  bucket: SeverityBucket,
  disruption_text: string | null,
  segments: RouteSegment[] = [],
): LineStatus {
  return {
    line_id,
    disruption_text,
    status: [makeEntry(bucket, bucket, segments)],
  };
}

function multiEntryLine(
  line_id: string,
  disruption_text: string | null,
  entries: { bucket: SeverityBucket; description: string; segments?: RouteSegment[] }[],
): LineStatus {
  return {
    line_id,
    disruption_text,
    status: entries.map((e) => makeEntry(e.bucket, e.description, e.segments ?? [])),
  };
}

describe('StatusView — headline and count', () => {
  it('headlines the disruption count and lists disrupted worst-first', () => {
    render(StatusView, {
      statuses: [
        line('victoria', 'GoodService', null),
        line('central', 'MinorDelays', 'Minor delays'),
        line('bakerloo', 'Closed', 'Suspended'),
      ],
    });
    expect(screen.getByText(/2 disruptions/i)).toBeTruthy();
    const rows = Array.from(document.querySelectorAll('.statusview__line')).map((e) =>
      e.textContent?.trim(),
    );
    expect(rows[0]).toMatch(/bakerloo/i);
    expect(rows[1]).toMatch(/central/i);
  });

  it('says "All lines good" when nothing is wrong', () => {
    render(StatusView, { statuses: [line('victoria', 'GoodService', null)] });
    expect(screen.getByText(/all lines good/i)).toBeTruthy();
  });

  it('shows the freshness label when provided', () => {
    render(StatusView, {
      statuses: [line('victoria', 'GoodService', null)],
      updatedLabel: '3 min ago',
    });
    expect(screen.getByText(/updated 3 min ago/i)).toBeTruthy();
  });

  it('refresh button calls onRefresh', async () => {
    const onRefresh = vi.fn();
    render(StatusView, { statuses: [line('victoria', 'GoodService', null)], onRefresh });
    await fireEvent.click(screen.getByRole('button', { name: /refresh/i }));
    expect(onRefresh).toHaveBeenCalledOnce();
  });

  it('omits the refresh button when no handler is given', () => {
    render(StatusView, { statuses: [line('victoria', 'GoodService', null)] });
    expect(screen.queryByRole('button', { name: /refresh/i })).toBeNull();
  });
});

describe('StatusView — TfL-style disrupted rows', () => {
  it('renders a left colour stripe and bold line name for each disrupted line', () => {
    render(StatusView, {
      statuses: [line('bakerloo', 'Closed', 'Suspended')],
    });
    // Left stripe element
    const stripe = document.querySelector('.statusview__stripe');
    expect(stripe).not.toBeNull();
    // Line name appears
    expect(document.querySelector('.statusview__line')?.textContent).toMatch(/bakerloo/i);
  });

  it('a line with two StatusEntries shows both severity sub-headlines', () => {
    render(StatusView, {
      statuses: [
        multiEntryLine('central', 'Severe disruption', [
          { bucket: 'SevereDelays', description: 'Severe delays' },
          { bucket: 'PartClosure', description: 'Part suspended' },
        ]),
      ],
    });
    // Both entry descriptions should appear as sub-headlines
    expect(screen.getByText(/severe delays/i)).toBeTruthy();
    expect(screen.getByText(/part suspended/i)).toBeTruthy();
  });

  it('shows "A ↔ B" rows when entry has affected_segments', () => {
    render(StatusView, {
      statuses: [
        line('jubilee', 'SevereDelays', 'Severe delays', [
          { from: 'Baker Street', to: 'Stratford' },
          { from: 'Wembley Park', to: 'Stanmore' },
        ]),
      ],
    });
    expect(screen.getByText(/baker street/i)).toBeTruthy();
    expect(screen.getByText(/stratford/i)).toBeTruthy();
    expect(screen.getByText(/wembley park/i)).toBeTruthy();
    expect(screen.getByText(/stanmore/i)).toBeTruthy();
    // The ↔ separator
    const segments = document.querySelectorAll('[data-testid="route-segment"]');
    expect(segments.length).toBe(2);
  });

  it('shows "Entire line" when entry has no affected_segments', () => {
    render(StatusView, {
      statuses: [line('victoria', 'Closed', 'Suspended')],
    });
    expect(screen.getByText(/entire line/i)).toBeTruthy();
  });

  it('shows "Entire line" when affected_segments is an empty array', () => {
    render(StatusView, {
      statuses: [line('northern', 'SevereDelays', 'Severe delays', [])],
    });
    expect(screen.getByText(/entire line/i)).toBeTruthy();
  });
});

describe('StatusView — disclosure chevron', () => {
  it('chevron starts collapsed (aria-expanded=false)', () => {
    render(StatusView, {
      statuses: [line('central', 'MinorDelays', 'Delays due to signal failure')],
    });
    const btn = document.querySelector(
      '[data-testid="details-toggle"]',
    ) as HTMLButtonElement | null;
    expect(btn).not.toBeNull();
    expect(btn!.getAttribute('aria-expanded')).toBe('false');
  });

  it('chevron click expands and shows disruption_text', async () => {
    render(StatusView, {
      statuses: [line('central', 'MinorDelays', 'Delays due to signal failure')],
    });
    const btn = document.querySelector('[data-testid="details-toggle"]')!;
    await fireEvent.click(btn);
    expect(btn.getAttribute('aria-expanded')).toBe('true');
    expect(screen.getByText(/delays due to signal failure/i)).toBeTruthy();
  });

  it('second click collapses again', async () => {
    render(StatusView, {
      statuses: [line('central', 'MinorDelays', 'Delays due to signal failure')],
    });
    const btn = document.querySelector('[data-testid="details-toggle"]')!;
    await fireEvent.click(btn);
    await fireEvent.click(btn);
    expect(btn.getAttribute('aria-expanded')).toBe('false');
    expect(screen.queryByText(/delays due to signal failure/i)).toBeNull();
  });

  it('chevron is omitted when disruption_text is null', () => {
    render(StatusView, {
      statuses: [line('victoria', 'Closed', null)],
    });
    // No toggle button when there's nothing to expand.
    expect(document.querySelector('[data-testid="details-toggle"]')).toBeNull();
  });
});

describe('StatusView — footer and empty state', () => {
  it('shows "Good service on all other lines" footer when some lines are disrupted', () => {
    render(StatusView, {
      statuses: [line('bakerloo', 'Closed', 'Suspended'), line('victoria', 'GoodService', null)],
    });
    expect(screen.getByText(/good service on all other lines/i)).toBeTruthy();
    // The old chip enumeration of healthy lines must NOT appear
    expect(document.querySelector('.statusview__healthy')).toBeNull();
  });

  it('shows "Service status unavailable." when statuses is empty', () => {
    render(StatusView, { statuses: [] });
    expect(screen.getByText(/service status unavailable\./i)).toBeTruthy();
  });
});

// @vitest-environment happy-dom
/**
 * DOM tests for StatusPanel — the worst-first, calm-state replacement for the
 * marquee ticker. Covers: good-service empty state, worst-first disrupted
 * ordering, and the partial-failure note.
 */
import { describe, expect, it } from 'vitest';
import { render, screen } from '@testing-library/svelte';
import type { LineStatus, SeverityBucket } from '$lib/ipc/types.js';
import StatusPanel from '../../components/StatusPanel.svelte';

function line(line_id: string, bucket: SeverityBucket, disruption_text: string | null): LineStatus {
  return {
    line_id,
    disruption_text,
    status: [{ severity: 0, description: bucket, bucket }],
  };
}

describe('StatusPanel', () => {
  it('shows the calm good-service state when all lines are healthy', () => {
    render(StatusPanel, {
      statuses: [line('victoria', 'GoodService', null), line('northern', 'GoodService', null)],
    });
    expect(screen.getByText(/good service across the network/i)).toBeTruthy();
    // No disruption rows.
    expect(document.querySelector('.status__list')).toBeNull();
  });

  it('treats empty statuses as good service (not a blank pane)', () => {
    render(StatusPanel, { statuses: [] });
    expect(screen.getByText(/good service across the network/i)).toBeTruthy();
  });

  it('lists disrupted lines worst-first and omits healthy lines', () => {
    render(StatusPanel, {
      statuses: [
        line('victoria', 'GoodService', null),
        line('central', 'MinorDelays', 'Minor delays'),
        line('bakerloo', 'Closed', 'Suspended'),
      ],
    });
    const rows = Array.from(document.querySelectorAll('.status__line')).map((el) =>
      el.textContent?.trim(),
    );
    // Closed (worst) before MinorDelays; Victoria (good) absent.
    expect(rows[0]).toMatch(/bakerloo/i);
    expect(rows[1]).toMatch(/central/i);
    expect(rows).toHaveLength(2);
  });

  it('prefers TfL disruption text, falling back to the bucket label', () => {
    render(StatusPanel, {
      statuses: [
        line('central', 'SevereDelays', 'Severe delays between X and Y'),
        line('jubilee', 'MinorDelays', null), // no text → bucket label
      ],
    });
    expect(screen.getByText(/severe delays between x and y/i)).toBeTruthy();
    expect(screen.getByText(/^Minor delays$/i)).toBeTruthy();
  });

  it('carries a severity bucket on each row for tier styling', () => {
    render(StatusPanel, { statuses: [line('bakerloo', 'Closed', 'Suspended')] });
    const dot = document.querySelector('.status__dot');
    expect(dot?.getAttribute('data-bucket')).toBe('Closed');
  });

  it('shows the partial note when some lines could not be checked', () => {
    render(StatusPanel, {
      statuses: [line('victoria', 'GoodService', null)],
      partial: true,
    });
    expect(screen.getByText(/couldn't be checked/i)).toBeTruthy();
  });

  it('omits the partial note when nothing failed', () => {
    render(StatusPanel, { statuses: [line('victoria', 'GoodService', null)] });
    expect(screen.queryByText(/couldn't be checked/i)).toBeNull();
  });

  it('says "Good service across the network" (network-wide copy) when all lines are healthy', () => {
    render(StatusPanel, { statuses: [line('victoria', 'GoodService', null)] });
    expect(screen.getByText(/good service across the network/i)).toBeTruthy();
  });
});

// @vitest-environment happy-dom
/**
 * DOM tests for StatusView — the full Service-status view (iOS-tab equivalent):
 * headline count, disrupted lines worst-first, an "Other lines — good service"
 * section, freshness line, and manual refresh.
 */
import { describe, expect, it, vi } from 'vitest';
import { render, screen, fireEvent } from '@testing-library/svelte';
import type { LineStatus, SeverityBucket } from '$lib/ipc/types.js';
import StatusView from '../../components/StatusView.svelte';

function line(line_id: string, bucket: SeverityBucket, text: string | null): LineStatus {
  return { line_id, disruption_text: text, status: [{ severity: 0, description: bucket, bucket }] };
}

describe('StatusView', () => {
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
    expect(rows[0]).toMatch(/bakerloo/i); // Closed before MinorDelays
    expect(rows[1]).toMatch(/central/i);
  });

  it('lists healthy lines under "Other lines — good service"', () => {
    render(StatusView, {
      statuses: [
        line('central', 'SevereDelays', 'Severe delays'),
        line('victoria', 'GoodService', null),
      ],
    });
    expect(screen.getByText(/other lines — good service/i)).toBeTruthy();
    // Victoria appears as a healthy chip.
    const healthy = document.querySelector('.statusview__healthy');
    expect(healthy?.textContent).toMatch(/victoria/i);
  });

  it('says "All lines good" and uses the "Good service" label when nothing is wrong', () => {
    render(StatusView, { statuses: [line('victoria', 'GoodService', null)] });
    expect(screen.getByText(/all lines good/i)).toBeTruthy();
    expect(screen.getByText(/^good service$/i)).toBeTruthy();
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

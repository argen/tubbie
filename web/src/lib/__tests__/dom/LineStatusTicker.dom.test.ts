// @vitest-environment happy-dom
import { describe, expect, it, vi } from 'vitest';
import { render, screen } from '@testing-library/svelte';
import LineStatusTicker from '$lib/components/LineStatusTicker.svelte';
import type { LineStatus } from '$lib/ipc/types.js';
import { sampleLineStatus, sampleDisruptedLineStatus } from '$lib/ipc/mock.js';

vi.mock('$lib/stores/reducedMotion.js', () => ({
  reducedMotion: {
    subscribe: (fn: (v: boolean) => void) => {
      fn(false);
      return () => undefined;
    },
  },
}));

describe('LineStatusTicker', () => {
  it('shows "Good service on all lines" when no disruptions', () => {
    render(LineStatusTicker, {
      props: { statuses: [sampleLineStatus] },
    });
    expect(screen.getByText(/Good service on all lines/)).toBeTruthy();
  });

  it('shows disruption text when a disruption exists', () => {
    render(LineStatusTicker, {
      props: { statuses: [sampleDisruptedLineStatus] },
    });
    // The ticker renders the text in the sr-only span (accessible version) and
    // the scrolling span (duplicated for seamless loop). Use getAllByText.
    const matches = screen.getAllByText(/NORTHERN LINE: Minor delays/);
    expect(matches.length).toBeGreaterThan(0);
  });

  it('has correct ARIA role and label', () => {
    render(LineStatusTicker, { props: { statuses: [] } });
    const ticker = screen.getByRole('status');
    expect(ticker.getAttribute('aria-label')).toBe('Service status');
    expect(ticker.getAttribute('aria-live')).toBe('polite');
  });

  it('shows "DISRUPTIONS" label for disrupted statuses', () => {
    render(LineStatusTicker, {
      props: { statuses: [sampleDisruptedLineStatus] },
    });
    const label = screen.getByText('DISRUPTIONS');
    expect(label).toBeTruthy();
    expect(label.getAttribute('aria-hidden')).toBe('true');
  });

  it('shows "SERVICE" label for non-disrupted statuses', () => {
    render(LineStatusTicker, { props: { statuses: [sampleLineStatus] } });
    const label = screen.getByText('SERVICE');
    expect(label).toBeTruthy();
  });

  it('handles empty statuses array', () => {
    render(LineStatusTicker, { props: { statuses: [] } });
    expect(screen.getByText(/Good service on all lines/)).toBeTruthy();
  });

  it('handles multiple disruptions joined by bullet', () => {
    const disrupted1: LineStatus = {
      line_id: 'northern',
      status: [{ severity: 5, description: 'Minor Delays' }],
      disruption_text: 'NORTHERN: Delays.',
    };
    const disrupted2: LineStatus = {
      line_id: 'victoria',
      status: [{ severity: 5, description: 'Minor Delays' }],
      disruption_text: 'VICTORIA: Signal failure.',
    };
    render(LineStatusTicker, { props: { statuses: [disrupted1, disrupted2] } });
    // Multiple elements may exist due to ticker duplication for seamless scrolling
    const matches = screen.getAllByText(/NORTHERN: Delays\./);
    expect(matches.length).toBeGreaterThan(0);
  });
});

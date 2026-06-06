// @vitest-environment happy-dom
/**
 * DOM tests for StatusPanel — the bottom marquee / calm-state summary panel.
 *
 * Covers:
 *   - good-service all-lines: static "Good service across the network" (no marquee)
 *   - disrupted marquee: lists disrupted lines + "Good service on all other lines"
 *   - reduced-motion (writable store = true): static list, no marquee element
 *   - partial note
 */
import { describe, expect, it, vi, beforeEach } from 'vitest';
import { render, screen } from '@testing-library/svelte';
import type { LineStatus, SeverityBucket } from '$lib/ipc/types.js';

// ---------------------------------------------------------------------------
// Module-level mock for reducedMotion store.
// vi.mock is hoisted so it runs before any import resolution.
// We use vi.hoisted() so the variable is available inside the factory.
// ---------------------------------------------------------------------------
const { mockReducedMotion } = vi.hoisted(() => {
  // eslint-disable-next-line @typescript-eslint/no-require-imports
  const { writable: w } = require('svelte/store') as typeof import('svelte/store');
  return { mockReducedMotion: w(false) };
});

vi.mock('$lib/stores/reducedMotion.js', () => ({
  reducedMotion: mockReducedMotion,
}));

// Import AFTER the mock is declared (hoisting ensures the mock is in place).
import StatusPanel from '../../components/StatusPanel.svelte';

function line(line_id: string, bucket: SeverityBucket, disruption_text: string | null): LineStatus {
  return {
    line_id,
    disruption_text,
    status: [{ severity: 0, description: bucket, bucket }],
  };
}

beforeEach(() => {
  // Reset to no-reduced-motion before each test.
  mockReducedMotion.set(false);
});

describe('StatusPanel — all-good state', () => {
  it('renders static "Good service across the network" with no marquee when all lines healthy', () => {
    render(StatusPanel, {
      statuses: [line('victoria', 'GoodService', null), line('northern', 'GoodService', null)],
    });
    expect(screen.getByText(/good service across the network/i)).toBeTruthy();
    // No marquee element when all good.
    expect(document.querySelector('[data-testid="status-marquee"]')).toBeNull();
  });

  it('treats empty statuses as all-good', () => {
    render(StatusPanel, { statuses: [] });
    expect(screen.getByText(/good service across the network/i)).toBeTruthy();
  });
});

describe('StatusPanel — disrupted marquee (reduced motion OFF)', () => {
  it('renders marquee with disrupted lines and "Good service on all other lines"', () => {
    render(StatusPanel, {
      statuses: [
        line('bakerloo', 'Closed', 'Suspended'),
        line('central', 'MinorDelays', 'Minor delays'),
        line('victoria', 'GoodService', null),
      ],
    });
    // The marquee wrapper should be present.
    const marquee = document.querySelector('[data-testid="status-marquee"]');
    expect(marquee).not.toBeNull();
    // Disrupted line names should appear inside the marquee.
    expect(marquee!.textContent).toMatch(/bakerloo/i);
    expect(marquee!.textContent).toMatch(/central/i);
    // The "good service" trailer should also appear in the marquee.
    expect(marquee!.textContent).toMatch(/good service on all other lines/i);
    // Healthy line (victoria) should NOT appear as a named disrupted item.
    const lineItems = Array.from(document.querySelectorAll('[data-testid="marquee-line"]'));
    const lineNames = lineItems.map((el) => el.textContent?.trim().toLowerCase());
    expect(lineNames.some((n) => n?.includes('victoria'))).toBe(false);
  });

  it('worst-first ordering: Closed before MinorDelays in marquee', () => {
    render(StatusPanel, {
      statuses: [
        line('central', 'MinorDelays', 'Minor delays'),
        line('bakerloo', 'Closed', 'Suspended'),
      ],
    });
    const marquee = document.querySelector('[data-testid="status-marquee"]');
    const text = marquee!.textContent ?? '';
    expect(text.indexOf('Bakerloo')).toBeLessThan(text.indexOf('Central'));
  });

  it('shows the status CATEGORY, not the long reason text', () => {
    // The bottom marquee must stay short: the line + its status bucket label
    // ("Minor delays"), NOT TfL's verbose reason prose (that lives in the
    // Status tab). Guards against regressing back to lineStatusLabel.
    render(StatusPanel, {
      statuses: [
        line('central', 'MinorDelays', 'Signal failure at Camden Town causing long delays'),
      ],
    });
    const text = document.querySelector('[data-testid="status-marquee"]')!.textContent ?? '';
    expect(text).toMatch(/minor delays/i); // the bucket category
    expect(text).not.toMatch(/signal failure/i); // NOT the verbose reason
  });
});

describe('StatusPanel — reduced motion (static list)', () => {
  it('renders static list (no marquee) when reducedMotion store is true', async () => {
    // Set reduced motion BEFORE rendering.
    mockReducedMotion.set(true);

    render(StatusPanel, {
      statuses: [line('bakerloo', 'Closed', 'Suspended'), line('victoria', 'GoodService', null)],
    });

    // Marquee element must NOT be present.
    expect(document.querySelector('[data-testid="status-marquee"]')).toBeNull();
    // Static list must be present.
    expect(document.querySelector('[data-testid="status-static-list"]')).not.toBeNull();
    // Disrupted line must appear.
    expect(screen.getByText(/bakerloo/i)).toBeTruthy();
    // "Good service on all other lines" must appear as a static item.
    expect(screen.getByText(/good service on all other lines/i)).toBeTruthy();
  });
});

describe('StatusPanel — partial prop', () => {
  it('shows partial note when partial=true', () => {
    render(StatusPanel, {
      statuses: [line('victoria', 'GoodService', null)],
      partial: true,
    });
    expect(screen.getByText(/couldn't be checked/i)).toBeTruthy();
  });

  it('omits partial note when partial=false', () => {
    render(StatusPanel, { statuses: [line('victoria', 'GoodService', null)] });
    expect(screen.queryByText(/couldn't be checked/i)).toBeNull();
  });
});

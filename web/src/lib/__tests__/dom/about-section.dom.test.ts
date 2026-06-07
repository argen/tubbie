// @vitest-environment happy-dom
/**
 * DOM test for the About settings section (Phase 4): version line + the
 * source/TfL links + attribution.
 */
import { describe, expect, it, vi } from 'vitest';
import { render, screen, waitFor } from '@testing-library/svelte';
import AboutSection from '../../components/AboutSection.svelte';

vi.mock('@tauri-apps/api/app', () => ({
  getVersion: () => Promise.resolve('1.2.3'),
}));

describe('AboutSection', () => {
  it('shows the app name and version', async () => {
    render(AboutSection);
    await waitFor(() => {
      expect(screen.getByTestId('about-version').textContent).toContain('1.2.3');
    });
    expect(screen.getByTestId('about-version').textContent).toContain('Tubbie');
  });

  it('links to source and TfL Open Data', () => {
    render(AboutSection);
    const source = screen.getByRole('link', { name: /source & releases/i }) as HTMLAnchorElement;
    const tfl = screen.getByRole('link', { name: /tfl open data/i }) as HTMLAnchorElement;
    expect(source.href).toContain('github.com/argen/tubbie');
    expect(tfl.href).toContain('tfl.gov.uk');
    // The href is kept for semantics (copy-link, screen readers), but the click
    // is intercepted and routed to the system browser via the opener plugin —
    // so there is no window.opener relationship and no target="_blank" to guard.
    // The behavioural "opens via opener" contract is pinned in
    // links-open-external.dom.test.ts.
    expect(source.getAttribute('target')).toBeNull();
  });

  it('carries the required TfL Open Data attribution', () => {
    render(AboutSection);
    expect(screen.getByText(/powered by tfl open data/i)).toBeTruthy();
  });
});

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
    // External links must be safe.
    expect(source.rel).toContain('noopener');
  });

  it('carries the required TfL Open Data attribution', () => {
    render(AboutSection);
    expect(screen.getByText(/powered by tfl open data/i)).toBeTruthy();
  });
});
